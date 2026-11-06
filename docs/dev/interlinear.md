# Interlinear lines (#143)

A Bible Tab can show **Interlinear lines** under each Tagged word. The Tab header's
**Interlinear** toggle (**Int.** in a narrow Panel) turns them on and off; the **⏷** menu next to
it picks the lines. Only Bibles with Tagged words show the controls.

| Bible | Kind | Lines offered, top to bottom | Shown until the user picks |
|---|---|---|---|
| English (or other modern language) with Strong's numbers, e.g. KJV | reverse interlinear | Lemma, transliteration, Strong's number, Morphology, Gloss | Lemma, transliteration, Strong's number |
| Hebrew, Aramaic or Greek, e.g. TAHOT, TAGNT | interlinear | Gloss, transliteration, Morphology, Strong's number | Gloss, transliteration |

The kind comes from the Bible's language (`InterlinearKind::for_language`, the same test the
Key words section uses for an original-language Bible).

## Where each line comes from

`obs_engine::interlinear::cell_texts` gives each line's text for one Tagged word, from the
word's main morpheme (the first part below the STEPBible affix numbers H9001/G9001, as Word
Info picks it):

- **Lemma**: the morpheme's own Lemma (TAHOT, TAGNT), else the headword of the first Lexicon in
  Preferred order with an entry for the number (KJV+Strong's tags only numbers).
- **Transliteration**: of the word as written in an original-language Bible; of the Lemma in a
  reverse interlinear (`obs_model::transliterate`).
- **Strong's number**: the main morpheme's.
- **Morphology**: the code read as short plain English (`morph::parse(..).short()`); a code that
  does not parse (KJV's `strongMorph:TH8804`) shows nothing rather than the code.
- **Gloss**: every morpheme's gloss in order in an original-language Bible ("in beginning"), as
  Word Info shows it; in a reverse interlinear the morpheme's gloss, else the Lexicons' brief
  gloss (`cell_gloss`: the first excerpt of 28 characters or fewer, else the first one's opening
  phrase, cut with "…").

The UI's `InterlinearSource` (`crates/ui-egui/src/interlinear.rs`) holds the Lexicons and
memoizes each number's lookup, so the layout workers look each Strong's number up once. Lexicons
are loaded only when a reverse interlinear shows a line that needs them.

## Layout

`obs_layout::add_interlinear` adds an `Interlinear` (rows, size and each word's cells as styled
runs) to a block's `Paragraph`; `layout` then:

- never breaks a Tagged word, and pads it on both sides so it is as wide as its widest line plus
  a gap, centred in that column;
- lays each cell out on its own in its own direction, so a Hebrew Lemma under an English word and
  an English Gloss under a Hebrew word both read correctly while the paragraph's bidi order puts
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

`Tab::interlinear: InterlinearLines { on, chosen }` is kept per Tab, saved in Layouts and the
Last session (`TabSnapshot::interlinear`, left out while it is the default). The command
`tab.interlinear {tab, on?, line?, shown?}` toggles the lines, or picks (`shown: true`) or drops
a line; the last line picked cannot be dropped. Turning lines on or off, or picking one, lays
the Tab's text out again and keeps its Current location (`ReadingView::set_interlinear`).

## Performance (#19)

A block with lines is estimated with room for its rows (`estimate_height` widens the text by
`INTERLINEAR_WIDEN` and adds the rows to each line) and counts six times dearer against the
UI thread's layout budget. The bench's Bible tags about seven words in ten with a Hebrew
Lemma and shows the lines in one of its four panes (`BenchConfig::interlinear_panes`;
`openbible --bench --interlinear-panes N`).
