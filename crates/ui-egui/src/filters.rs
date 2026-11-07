//! Visual filters (#211) in a reading pane: the words of each visible block that a Tab's
//! filters match, found on a worker thread and drawn in the filters' looks.
//!
//! - Matching is [`obs_engine::block_hits`] (the Search query read Verse by Verse, as the
//!   Search Tool reads it, on the block's text and Tagged words). A pane asks for the blocks it shows and a few on either side;
//!   each block is matched once per set of filters and kept while it is near the view.
//! - Each block's match rectangles are taken from its layout once (and again only when it is
//!   laid out again), so a scrolling frame only moves them.
//! - A match carries its filter's [`obs_engine::StyleAppearance`]: the painter resolves its
//!   colour from the Page tone's palette each frame, so a theme switch needs no relayout.
//! - Strong's alignment (ADR 0015): a Bible not in Hebrew, Aramaic or Greek takes the `@…`
//!   and `lemma:` terms from the first original-language Bible in Preferred order that has
//!   each Verse ([`OriginalBible`]), loaded on the worker the first time it is needed.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};

use obs_engine::{
    AlignedWords, FilterMatcher, StyleAppearance, VerseSlice, VisualFilter, block_hits,
};
use obs_model::{ResourceType, SpanKind};
use obs_scripture::{Passage, StrongsLanguage, Testament, Verse};

use crate::doc::ReadingDoc;
use crate::pane::FilterRects;
use crate::selection::DocPos;
use crate::view::ReadingView;

/// Blocks matched beyond each end of the view, so text scrolled into view is ready.
const READ_AHEAD_BLOCKS: usize = 4;

/// An original-language Bible the filters align through.
pub struct OriginalBible {
    pub language: StrongsLanguage,
    /// Its `.obr`, loaded on the worker when not given loaded.
    pub path: Option<PathBuf>,
    doc: OnceLock<Option<Arc<ReadingDoc>>>,
}

impl OriginalBible {
    pub fn new(
        language: StrongsLanguage,
        path: Option<PathBuf>,
        loaded: Option<Arc<ReadingDoc>>,
    ) -> Self {
        let doc = OnceLock::new();
        if loaded.is_some() {
            let _ = doc.set(loaded);
        }
        Self {
            language,
            path,
            doc,
        }
    }

    /// Its text, loaded the first time (none when it cannot be read).
    fn doc(&self) -> Option<&Arc<ReadingDoc>> {
        self.doc
            .get_or_init(|| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let path = self.path.as_ref()?;
                    ReadingDoc::open(path).ok().map(Arc::new)
                }
                #[cfg(target_arch = "wasm32")]
                None
            })
            .as_ref()
    }

    /// The morphemes of each Tagged word of its Verses at a Standard Passage, in order: none
    /// when it lacks the Passage or tags none of it.
    fn words_at(&self, standard: Passage) -> AlignedWords {
        let Some(doc) = self.doc() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for i in doc.verses_for_standard(standard) {
            let (Some(v), Some(unit)) = (
                doc.verses.get(i),
                doc.verse_unit(i).and_then(|u| doc.units.get(u)),
            ) else {
                continue;
            };
            for s in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &s.kind
                    && v.start <= s.start
                    && s.end <= v.end
                    && !parts.is_empty()
                {
                    out.push(parts.clone());
                }
            }
        }
        out
    }
}

/// What a pane's filters are matched with, shared by every job for one set of filters.
pub struct FilterSource {
    filters: Vec<VisualFilter>,
    /// Lexicons for `lemma:` terms typed in transliteration.
    lexicons: Vec<PathBuf>,
    /// Original-language Bibles in Preferred order.
    originals: Vec<OriginalBible>,
    matchers: OnceLock<Vec<FilterMatcher>>,
}

impl FilterSource {
    pub fn new(
        filters: Vec<VisualFilter>,
        lexicons: Vec<PathBuf>,
        originals: Vec<OriginalBible>,
    ) -> Self {
        Self {
            filters,
            lexicons,
            originals,
            matchers: OnceLock::new(),
        }
    }

    /// The filters compiled (a filter that no longer compiles is left out).
    fn matchers(&self) -> &[FilterMatcher] {
        self.matchers.get_or_init(|| {
            let lookup = LexiconLookup(&self.lexicons);
            self.filters
                .iter()
                .filter_map(|f| FilterMatcher::new(f, &lookup).ok())
                .collect()
        })
    }

    /// The original-language words at a Standard Passage: from the first Bible in Preferred
    /// order in its Testament's language that has them.
    fn aligned(&self, standard: Passage) -> AlignedWords {
        let language = match standard.start().book.testament() {
            Testament::Old => StrongsLanguage::Hebrew,
            Testament::New => StrongsLanguage::Greek,
        };
        for o in self.originals.iter().filter(|o| o.language == language) {
            let words = o.words_at(standard);
            if !words.is_empty() {
                return words;
            }
        }
        Vec::new()
    }
}

/// `lemma:` transliterations through the Lexicons' transliteration index.
// wasm has no Lexicon files to read: the paths go unused there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct LexiconLookup<'a>(&'a [PathBuf]);

impl obs_search::LemmaLookup for LexiconLookup<'_> {
    #[cfg(not(target_arch = "wasm32"))]
    fn lemmas_for_transliteration(&self, translit: &str) -> Vec<String> {
        use obs_store::ResourceStore;
        let mut out = Vec::new();
        for path in self.0 {
            if let Ok(reader) = obs_store::ObrReader::open(path)
                && let Ok(found) = reader.lemmas_for_transliteration(translit)
            {
                out.extend(found);
            }
        }
        out
    }

    #[cfg(target_arch = "wasm32")]
    fn lemmas_for_transliteration(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Whether a Bible in `language` is in Hebrew, Aramaic or Greek (it has its own Morphology).
pub fn is_original(language: &str) -> bool {
    [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
        .into_iter()
        .any(|l| obs_engine::key_words::is_original_language(language, l))
}

/// Matching jobs, run one at a time on their own thread (inline on wasm).
type Work = Box<dyn FnOnce() + Send>;

fn worker() -> Option<Sender<Work>> {
    static WORKER: OnceLock<Mutex<Option<Sender<Work>>>> = OnceLock::new();
    WORKER
        .get_or_init(|| Mutex::new(spawn_worker()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_worker() -> Option<Sender<Work>> {
    let (tx, rx) = channel::<Work>();
    std::thread::Builder::new()
        .name("obs-filters".into())
        .spawn(move || {
            while let Ok(work) = rx.recv() {
                work();
            }
        })
        .ok()
        .map(|_| tx)
}

#[cfg(target_arch = "wasm32")]
fn spawn_worker() -> Option<Sender<Work>> {
    None
}

fn submit(work: Work) {
    match worker() {
        Some(tx) => {
            if let Err(e) = tx.send(work) {
                (e.0)();
            }
        }
        None => work(),
    }
}

/// The Bible Verses of `doc` inside `bytes` of unit `unit`, and up to `context` more of the
/// unit's Verses on either side (for `WITHIN n verses`), in order, with their Standard
/// Passages.
fn verse_slices(
    doc: &ReadingDoc,
    unit: usize,
    bytes: &std::ops::Range<usize>,
    context: usize,
) -> Vec<VerseSlice> {
    if doc.manifest.resource_type != ResourceType::Bible {
        return Vec::new();
    }
    let in_unit: Vec<usize> = (0..doc.verses.len())
        .filter(|&i| doc.verse_unit(i) == Some(unit))
        .collect();
    let inside = |i: usize| {
        let v = &doc.verses[i];
        (v.start as usize) < bytes.end && bytes.start < v.end as usize
    };
    let (Some(first), Some(last)) = (
        in_unit.iter().position(|&i| inside(i)),
        in_unit.iter().rposition(|&i| inside(i)),
    ) else {
        return Vec::new();
    };
    let wanted = first.saturating_sub(context)..(last + context + 1).min(in_unit.len());
    let mut out = Vec::new();
    for &i in &in_unit[wanted] {
        let v = &doc.verses[i];
        let range = v.start as usize..v.end as usize;
        let standard = match &doc.verse_map {
            Some(map) => map.to_standard(v.verse),
            None => Passage::new(
                v.verse,
                Verse::new(v.verse.book, v.verse.chapter, v.last.max(v.verse.verse)),
            )
            .ok(),
        };
        out.push(VerseSlice { range, standard });
    }
    out
}

/// One match in a block: the look it is drawn in and the bytes it covers.
type BlockMatch = (StyleAppearance, DocPos, DocPos);

/// What `source`'s filters match in block `index` of `doc`.
fn block_matches(doc: &ReadingDoc, index: usize, source: &FilterSource) -> Vec<BlockMatch> {
    let matchers = source.matchers();
    let Some(block) = doc.blocks.get(index) else {
        return Vec::new();
    };
    let Some(unit) = doc.units.get(block.unit) else {
        return Vec::new();
    };
    if matchers.is_empty() {
        return Vec::new();
    }
    let range = block.source.range.clone();
    let context = matchers
        .iter()
        .map(FilterMatcher::context_verses)
        .max()
        .unwrap_or(0);
    let verses = verse_slices(doc, block.unit, &range, context);
    let align = !verses.is_empty()
        && !is_original(&doc.manifest.language)
        && matchers.iter().any(FilterMatcher::needs_alignment)
        && doc.has_tagged_words();
    let mut original = |p: Passage| source.aligned(p);
    block_hits(matchers, unit, range, &verses, align, &mut original)
        .into_iter()
        .filter_map(|h| {
            let m = matchers.get(h.filter)?;
            let at = |byte| DocPos {
                unit: block.unit,
                byte,
            };
            Some((m.appearance, at(h.range.start), at(h.range.end)))
        })
        .collect()
}

/// A block's matches, for one generation of a painter's filters.
type Reply = (u64, usize, Arc<Vec<BlockMatch>>);

/// One pane's Visual filters: the matches of each block found so far, and those being found.
pub struct FilterPainter {
    /// What the matches are for (the filters, the Bibles aligned through, the document).
    key: Option<u64>,
    source: Option<Arc<FilterSource>>,
    done: HashMap<usize, Arc<Vec<BlockMatch>>>,
    /// Each block's match rectangles (its top at 0), for the layout they were taken from.
    rects: HashMap<usize, (usize, Arc<Vec<FilterRects>>)>,
    pending: HashSet<usize>,
    /// Bumped by each new key, so replies for an older one are dropped.
    generation: u64,
    /// Blocks drawn last time [`Self::marks`] ran whose matches were not found yet.
    missing: usize,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl Default for FilterPainter {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            key: None,
            source: None,
            done: HashMap::new(),
            rects: HashMap::new(),
            pending: HashSet::new(),
            generation: 0,
            missing: 0,
            tx,
            rx,
        }
    }
}

impl std::fmt::Debug for FilterPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterPainter")
            .field("key", &self.key)
            .field("done", &self.done.len())
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl FilterPainter {
    /// Match with `wanted`'s source (none: no filters) when its key differs from the one
    /// matched so far; otherwise keep what was found.
    pub fn set(&mut self, wanted: Option<(u64, FilterSource)>) {
        let key = wanted.as_ref().map(|(k, _)| *k);
        if self.key == key {
            return;
        }
        self.key = key;
        self.generation += 1;
        self.done.clear();
        self.rects.clear();
        self.pending.clear();
        self.source = wanted.map(|(_, source)| Arc::new(source));
    }

    /// Whether some block's matches are still being found.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// How many blocks in view were drawn without their matches the last time.
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// The rectangles of every match in the blocks in view (a viewport `height` tall), in
    /// text coordinates, asking for the matches of the blocks not matched yet and of a few
    /// beyond the view. `ctx` repaints when a block's matches are found. A block's rectangles
    /// are worked out once for each layout of it, so scrolling only moves them.
    pub fn rects(
        &mut self,
        view: &ReadingView,
        height: f32,
        ctx: Option<&egui::Context>,
    ) -> Vec<FilterRects> {
        for (generation, block, found) in self.rx.try_iter() {
            if generation == self.generation {
                self.pending.remove(&block);
                self.done.insert(block, found);
            }
        }
        self.missing = 0;
        let Some(source) = self.source.clone() else {
            return Vec::new();
        };
        let shown = view.laid_in_view(height);
        let (Some(first), Some(last)) = (shown.first().map(|s| s.0), shown.last().map(|s| s.0))
        else {
            return Vec::new();
        };
        let blocks = view.doc.blocks.len();
        let around =
            first.saturating_sub(READ_AHEAD_BLOCKS)..(last + 1 + READ_AHEAD_BLOCKS).min(blocks);
        // The blocks in view first, then the ones around them.
        let order = shown
            .iter()
            .map(|s| s.0)
            .chain(around.filter(|i| !(first..=last).contains(i)));
        for block in order {
            if self.done.contains_key(&block) || !self.pending.insert(block) {
                continue;
            }
            let doc = Arc::clone(&view.doc);
            let source = Arc::clone(&source);
            let tx = self.tx.clone();
            let generation = self.generation;
            let ctx = ctx.cloned();
            submit(Box::new(move || {
                let found = Arc::new(block_matches(&doc, block, &source));
                if tx.send((generation, block, found)).is_ok()
                    && let Some(ctx) = ctx
                {
                    ctx.request_repaint();
                }
            }));
        }
        // Keep only the blocks near the view: a whole Bible's matches are never held.
        let keep = first.saturating_sub(4 * READ_AHEAD_BLOCKS)..last + 1 + 4 * READ_AHEAD_BLOCKS;
        self.done.retain(|i, _| keep.contains(i));
        self.rects.retain(|i, _| keep.contains(i));
        let mut out = Vec::new();
        for (index, top, laid) in &shown {
            let Some(found) = self.done.get(index) else {
                self.missing += 1;
                continue;
            };
            let layout = Arc::as_ptr(laid) as usize;
            let rects = match self.rects.get(index) {
                Some((l, r)) if *l == layout => Arc::clone(r),
                _ => {
                    let ranges: Vec<(DocPos, DocPos)> =
                        found.iter().map(|(_, a, b)| (*a, *b)).collect();
                    let rects =
                        crate::selection::block_range_rects(&view.doc, *index, laid, &ranges);
                    let r: Arc<Vec<FilterRects>> = Arc::new(
                        found
                            .iter()
                            .zip(rects)
                            .map(|((look, _, _), rects)| (*look, rects))
                            .collect(),
                    );
                    self.rects.insert(*index, (layout, Arc::clone(&r)));
                    r
                }
            };
            out.extend(rects.iter().map(|(look, rects)| {
                (
                    *look,
                    rects
                        .iter()
                        .map(|[x0, y0, x1, y1]| [*x0, y0 + top, *x1, y1 + top])
                        .collect(),
                )
            }));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bench's `filters` scenario measures Strong's alignment, not an empty filter: its
    /// English Bible's imperatives come from the synthetic Hebrew Bible.
    #[test]
    fn the_bench_bible_aligns_imperatives_through_the_synthetic_hebrew_bible() {
        let english = crate::bench::synthetic_bible();
        let hebrew = crate::bench::synthetic_hebrew_bible();
        let imperatives: Vec<VisualFilter> = obs_engine::built_in_filters()
            .into_iter()
            .filter(|f| f.id == "imperatives")
            .collect();
        let aligned = FilterSource::new(
            imperatives.clone(),
            vec![],
            vec![OriginalBible::new(
                StrongsLanguage::Hebrew,
                None,
                Some(hebrew),
            )],
        );
        let unaligned = FilterSource::new(imperatives, vec![], vec![]);
        let count = |source: &FilterSource| -> usize {
            (0..20)
                .map(|i| block_matches(&english, i, source).len())
                .sum()
        };
        assert!(count(&aligned) > 20, "{}", count(&aligned));
        assert_eq!(count(&unaligned), 0);
    }

    /// Manual check against the built Starter set (`cargo xtask starter` writes `dist/starter`;
    /// skips when it is absent): "imperatives in Romans 12" in the real TAGNT and KJV+Strong's.
    #[test]
    #[ignore = "reads dist/starter/tagnt.obr and kjv-strongs.obr from a Starter build"]
    fn starter_imperatives_in_romans_12() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/starter");
        let (tagnt, kjv) = (dir.join("tagnt.obr"), dir.join("kjv-strongs.obr"));
        if !tagnt.exists() || !kjv.exists() {
            eprintln!("no Starter build; skipped");
            return;
        }
        let tagnt = Arc::new(ReadingDoc::open(&tagnt).unwrap());
        let kjv = Arc::new(ReadingDoc::open(&kjv).unwrap());
        let mut filter = obs_engine::built_in_filters()
            .into_iter()
            .find(|f| f.id == "imperatives")
            .unwrap();
        filter.scope = "Rom 12".into();
        let rom_12 = |doc: &ReadingDoc| -> Vec<usize> {
            let verses: Vec<(Option<usize>, std::ops::Range<usize>)> = (doc.verses.iter())
                .enumerate()
                .filter(|(_, v)| {
                    v.verse.book == obs_scripture::Book::Romans && v.verse.chapter == 12
                })
                .map(|(i, v)| (doc.verse_unit(i), v.start as usize..v.end as usize))
                .collect();
            (0..doc.blocks.len())
                .filter(|i| {
                    let b = &doc.blocks[*i];
                    let r = &b.source.range;
                    verses
                        .iter()
                        .any(|(u, v)| *u == Some(b.unit) && v.start < r.end && r.start < v.end)
                })
                .collect()
        };
        for (name, doc) in [("TAGNT", &tagnt), ("KJV", &kjv)] {
            let source = FilterSource::new(
                vec![filter.clone()],
                vec![],
                vec![OriginalBible::new(
                    StrongsLanguage::Greek,
                    None,
                    Some(Arc::clone(&tagnt)),
                )],
            );
            let started = std::time::Instant::now();
            let mut words = Vec::new();
            let blocks = rom_12(doc);
            for i in &blocks {
                for (_, a, b) in block_matches(doc, *i, &source) {
                    words.push(doc.units[a.unit].text[a.byte..b.byte].to_string());
                }
            }
            println!(
                "{name}: {} words in {} blocks ({:?}): {}",
                words.len(),
                blocks.len(),
                started.elapsed(),
                words.join(", ")
            );
            assert!(!words.is_empty(), "{name}");
        }
    }
}
