# Interlinear lines (#143)

A Bible Tab can show **Interlinear lines** under each Tagged word. The Tab header's
**Interlinear** toggle (**Int.** in a narrow Panel) turns them on and off; the **⏷** menu next to
it picks the lines. Only Bibles with Tagged words show the controls.

| Bible | Kind | Lines offered, top to bottom | Shown until the user picks |
|---|---|---|---|
| English (or other modern language) with Strong's numbers, e.g. KJV, BSB | reverse interlinear | Lemma, transliteration, Strong's number, Morphology, Gloss | Lemma, transliteration, Gloss |
| Hebrew, Aramaic or Greek, e.g. TAHOT, TAGNT | interlinear | Gloss, transliteration, Morphology, Strong's number | Gloss, transliteration |

The kind comes from the Bible's language (`InterlinearKind::for_language`, the same test the
Key words section uses for an original-language Bible).

## Where each line comes from

`obs_engine::interlinear::cell_texts` gives each line's text for one Tagged word, from the
word's main morpheme (the first part below the STEPBible affix numbers H9001/G9001, as Word
Info picks it). A Lemma or Gloss is looked for in order: the Bible's own tagging, the Lexicons
in Preferred order, then the **Built-in Strong's index** (#243), so a Strong's-tagged English
Bible alone (BSB, KJV) still fills its lines:

- **Lemma**: the morpheme's own Lemma (TAHOT, TAGNT; BSB carries the accented written Hebrew or
  Greek word), else the headword of the first Lexicon with an entry for the number, else the
  index's. It shows without cantillation, sof pasuq or paragraph marks (`הָאָֽרֶץ׃` → `הָאָרֶץ`).
- **Transliteration**: of the word as written in an original-language Bible; of the Lemma in a
  reverse interlinear. It is the academic one (`obs_model::transliterate_academic`, SBL-style:
  `bᵊrēʾšîṯ`, `haššāmayim`, `archē`; maqaf as a hyphen). Search keeps the ASCII
  `obs_model::transliterate`.
- **Strong's number**: the main morpheme's.
- **Morphology**: the code read as short plain English (`morph::parse(..).short()`); a code that
  does not parse (KJV's `strongMorph:TH8804`) shows nothing rather than the code. It comes only
  from the Bible's own tagging.
- **Gloss**: every morpheme's gloss in order in an original-language Bible ("in beginning"), as
  Word Info shows it; else (and in a reverse interlinear, after the morpheme's own gloss) the
  Lexicons' brief gloss (`cell_gloss`: the first excerpt of 28 characters or fewer, else the
  first one's opening phrase, cut with "…"), else the index's.

The **Built-in Strong's index** (`obs_engine::strongs_index`, data in
`crates/engine/assets/strongs/index.tsv`) maps every Strong's number, extended ones included
(`H1254A`; an unlisted extended number falls back to its base), to a lemma without accents and
a brief gloss. `cargo xtask strongs-index` regenerates it from the TBESH and TBESG files
`starter/sources.toml` pins (their sha256s are named in the file's header and checked by a
test). A base number with several senses takes the sense its English word uses (H7225 under
"In the beginning" → "beginning"), and a slash gloss the alternative used ("in/on/among" under
"In" → "in"). See ADR 0015.

The UI's `InterlinearSource` (`crates/ui-egui/src/interlinear.rs`) holds the Lexicons and
memoizes each number's lookup, so the layout workers look each Strong's number up once. Lexicons
are loaded only when the lines need them (`needs_lexicons`): a reverse interlinear's Lemma,
transliteration or Gloss, an original-language Bible's Lemma or Gloss. A source is rebuilt
when the lines, the Lexicons or their revisions change, and when a Lexicon it uses is replaced
in place (`Effect::Replaced`, which keeps the resource id and maybe the revision). It also notes
which lines have shown something under some word (`empty_lines`), for the hint below.

## Hint for lines with nothing to show

When a line the Tab shows has been empty under every word laid out so far (Morphology over an
English Bible, or a Lemma or Gloss neither the Lexicons nor the index have), the Tab shows an
inline hint above its text (`State::interlinear_hint`, text and buttons from
`obs_engine::interlinear::missing_lines_hint`): Morphology points at Hebrew and Greek Bibles
tagged with it, Lemma and Gloss at Lexicons, with the #135 get-resource buttons. Close runs
`tab.interlinear {tab, dismiss_hint: true}`; the hint then stays closed on that Tab.

## Layout

`obs_layout::add_interlinear` adds an `Interlinear` (rows, size and each word's cells as styled
runs) to a block's `Paragraph`; `layout` then:

- makes each Tagged word a column with the punctuation right against it (`said,`,
  `“Let there be`), never broken or spaced apart, its lines centred on the word;
- adds space at a column's edge only where its widest line, wider than its word, would come
  closer than the column gap (`COLUMN_GAP`, 0.6 × the lines' size) to the next column's lines,
  or reach under the next column's text or an untagged word beside it; at a line's start or end
  only what keeps the lines inside the line counts (a trailing space is not a neighbour). Lines
  narrower than their words leave the text exactly where it is without them;
- fills lines by measuring each candidate line as it will be drawn (`arrange_line`, edge
  padding included), so a short word with wide lines wraps instead of running past the pane;
- lays each cell out on its own as a bidi paragraph of its own (UAX #9, direction from its first
  strong letter), split where its face or resolved level changes and reordered by level, so a
  Hebrew Lemma under an English word, an English Gloss under a Hebrew word, and mixed cells
  (`אב (ab)`, a Hebrew word with digits) all read correctly while the paragraph's bidi order puts
  the columns right to left in Hebrew;
- gives every row the height of its tallest cell in the paragraph, so rows line up from line to
  line, and keeps `LaidLine::text_bottom` (the text's own band) apart from `bottom` (with rows).

Cells are drawn in `ColorRole::Interlinear` (the Note-marker tone of the palette).

## Selecting, copying, clicking and Annotations

- A cell hit-tests as `InlineObject::InterlinearCell { row }` anchored at its word's first byte
  (`LaidBlock::hit`); under the text, `caret_at` goes by x alone. A drag along the lines selects
  the words above them: a selection, and anything made from it, is always unit text.
- The selection highlight covers the selected words' cells too (`selection_rects`); Highlights,
  Note ranges and the hover underline use the text's band only (`range_rects` stops at
  `text_bottom`), so Annotations are never drawn over or anchored in the lines.
- Copy (Ctrl+C or the selection toolbar) puts the text, then one tab-separated row per Tagged
  word (`For\tγάρ\tgar\tG1063`), then the reference (`ReadingView::copy_text`).
- A click on a cell opens the word's right-click menu (#142) at the pointer, with the word as
  the selection.

## State

`Tab::interlinear: InterlinearLines { on, chosen, hint_dismissed }` is kept per Tab, saved in
Layouts and the Last session (`TabSnapshot::interlinear`, left out while it is the default). The
command `tab.interlinear {tab, on?, line?, shown?, dismiss_hint?}` toggles the lines, picks
(`shown: true`) or drops a line, or closes the hint for good (`dismiss_hint: true`); the last
line picked cannot be dropped. The choice is kept when the Tab switches to
another Bible (`tab.parallel`); a choice naming none of the new kind's lines (Lemma alone, then a
Hebrew or Greek Bible) reads as that kind's default lines, so the lines never go empty. Turning lines on or off, or picking one, lays
the Tab's text out again and keeps its Current location (`ReadingView::set_interlinear`).

## Performance (#19)

A block with lines is estimated with room for its rows (`estimate_height` widens the text by
`INTERLINEAR_WIDEN` and adds the rows to each line) and counts six times dearer against the
UI thread's layout budget. The bench's Bible tags about seven words in ten with a Hebrew
Lemma and shows the lines in one of its four panes (`BenchConfig::interlinear_panes`;
`openbible --bench --interlinear-panes N`).
