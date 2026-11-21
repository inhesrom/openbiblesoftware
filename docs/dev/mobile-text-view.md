# Mobile text view prototype (S07, #331)

The prototype that answers #331 for iOS: Resource text drawn by our own pipeline (ADR 0004)
in a hosted UIKit view, with native scrolling, selection, VoiceOver and Dynamic Type. Plan
`docs/plans/mobile-ios.md` A4 (option B, CPU tiles) and S07. It is throwaway-quality on
purpose: the Rust surface is marked `S07 prototype` and S13 replaces it; the Swift is rough.
What it must not be is a place where study rules live in Swift (ADR 0021 rule 3): every
label, colour, word or Verse decision, reference and count the screen shows comes from Rust.

The owner judges it on a real iPhone in S08 (the script is at the end). The numbers here are
simulator numbers and only indicative.

## Contract

**Rust (`crates/mobile/src/text_view.rs`, `obs-ffi` mirrors it):**

- **Content coordinates.** The text view is a `UIScrollView` whose content coordinates are
  the reader's page in points: x from the page's left edge, y from the document's top. Every
  rectangle the core returns (tiles, selection, caret, accessibility frames) is in them.
- **`reader_frame(reader, offset, width, height, scale, dark) -> ReaderFrame`.** The shell
  sends its content offset (clamped to the page; rubber-banding stays the scroll view's
  own). The reader scrolls its `ReadingView` by the difference from the position it last
  reported (the view stays anchored to a block, #14 rule 7), lays out what is in view and
  one viewport either side (`AROUND`; `ReadingView::visible_around`), and reports
  `position`, `total`, the page colour, the tiles to keep and where, and the Current
  location label. When blocks above the view were laid out at heights other than their
  estimates, `position` differs from the offset sent and the shell moves its offset there,
  so the text in view never jumps. After a jump or a new Text size the next frame ignores
  the shell's offset and the shell takes the core's position. A new width, scale, Page tone,
  Text size or system dark/light starts a new tile epoch.
- **Tiles: `reader_tile(reader, key, scale) -> TimedTile?`.** A tile is one band (at most
  `BAND` = 256 pt) of one block, keyed `TileKey { epoch, block, band }`, drawn by
  `obs_reading::compose_block_tile`: the block's layout and the band's pixels, wherever the
  block is. This is the bound the S03 review asked for: S03's `layout_tile` laid out
  everything from the Current location down, so its cost grew with `top`
  (`tile::tests::a_block_tile_costs_one_blocks_layout_wherever_the_block_is`,
  `text_view::tests::scrolling_moves_by_the_offset_and_a_tile_costs_one_block_wherever_it_is`).
  A key from an old epoch returns nothing. The shell keeps the images of the keys the last
  frame listed and drops the rest; the pixels are never cached in Rust.
- **Text input.** Positions are *text offsets*: each unit's bytes in document order, one
  more between units (`text_offset`, `text_end`). `closest_offset(x, y)`, `move_offset(offset,
  by)` (by characters, clamped), `enclosing_span(offset, kind)` where the core decides what
  each UIKit granularity covers (a word is the word under the offset, never a space; a
  sentence, line or paragraph is the Verse in a Bible, the block otherwise), `text_in`,
  `copy_text` (the text, then its reference: `John 3:16 (BSB)`), `span_rects` and
  `caret_rect` (the leading edge, right-to-left aware).
- **Accessibility: `accessibility_items(reader)`.** One element per Verse in a Bible (a
  Verse split across paragraphs is one element, its frame the union) and one per block
  without Verses (a heading, marked as one), in reading order, for the text laid out around
  the view. The label is the reference then the text: `Genesis 1:1. In the beginning God
  created the heavens and the earth.` The identifier is the Verse's OSIS id (`Gen.1.1`).
- **Tap: `tap(reader, x, y) -> StudySheetStub?`.** The core decides: the Verse under the
  point, its text and its cross-reference count across the library's Cross-reference sets
  (through the Bible's Verse map to the Standard versification), and, when the word under the
  point is a Tagged word, the word, its Strong's number, Lemma and Gloss (`ShownWord`, as
  Word Info builds it). The count's label (`12 cross-references`) is the core's.
- **Navigation.** `go_to(reader, text)` reads a typed reference in the Bible's own numbering
  with the Current location as context (`Jn 3:16`, `3:16`); its error is the parser's.
  `books(reader)` lists the Bible books the Resource has with their chapter counts, and
  `go_to_chapter(reader, book, chapter)` goes to one (the grid stub).
- **Appearance.** `page_tones()` (choices and labels from `PageTone::ALL`) and
  `set_page_tone(id)`, kept for the run only; `set_content_size(category)` maps UIKit's
  content size category to a Text size with `text_size_for_content_size`, a table in Rust so
  Android's font scale can use the same bounds (12–40 pt; Large is the desktop's 20 pt).
- **Debug Study set override.** `Paths::study_set`: a Starter library folder (`dist/starter`)
  whose Study set Resources (`mobile_set: "study"` in its `starter-manifest.json`) are added
  to the library read in place, not copied. The app sets it from the `OBS_STUDY_SET_DIR`
  environment variable in Debug builds only.

**Swift (`apps/ios/OpenBible`):**

- `ResourceTextView` (`UIScrollView`) asks for a frame on every `scrollViewDidScroll` and
  layout, moves its offset to the core's position, sets its content size to the core's
  total, its background to the core's page colour, and keeps one `CALayer` per listed tile
  at the listed place. A missing tile in view is drawn on the spot; the ones drawn ahead are
  drawn by `TileRenderer` on a serial queue (the core is behind one lock; only the copy of
  the pixels across the FFI and the image's preparation overlap the main thread) and placed
  when they arrive if the last frame still lists them.
- `TextCanvasView` is the content view: the tiles are its sublayers, it adopts `UITextInput`
  (every answer from the core, above) with a `UITextInteraction(for: .nonEditable)`, which
  gives the long-press selection, handles, loupe and the system edit menu (Copy), and it is
  the accessibility container of the core's elements.
- `ReaderScreen`: a reference field, a book and chapter grid stub, a menu with the Bible
  and Page tone pickers (the core's choices), "Show measurements" and the S06 About sheet,
  the Current location, and the Study sheet stub on a tap.

## Data

| Path | What |
|---|---|
| `crates/reading/src/view.rs` | `ReadingView::visible_around(above, height)` |
| `crates/reading/src/tile.rs` | `compose_block_tile`, and `compose_tile` sharing `draw_block` |
| `crates/mobile/src/text_view.rs` | the prototype surface above, and its tests |
| `apps/obs-ffi/src/lib.rs` | its mirror (`S07 prototype`) |
| `apps/ios/OpenBible/ResourceTextView.swift` | the scroll view, the tile queue, `TextViewMetrics` |
| `apps/ios/OpenBible/TextCanvasView.swift` | `UITextInput`, the tokenizer, the accessibility container |
| `apps/ios/OpenBible/ReaderModel.swift`, `ReaderScreen.swift` | the screen, the fling bench, cold start and footprint |
| `apps/ios/OpenBibleUITests/PrototypeTests.swift` | `testPrototypeSmoke`, `testAccessibilityAudit` |

**Debug launch arguments** (`simctl launch booted io.github.inhesrom.openbible.dev …`, or
the scheme's Arguments in Xcode): `-OBSResource tahot`, `-OBSPageTone sepia`, `-OBSGoTo
"Jn 3:16"`, `-OBSMeasurements YES` (the measurements strip), and the fling bench
`-OBSBenchUntil Leviticus [-OBSBenchSpeed 8000]`: from Genesis 1 it moves the content offset
`speed` points per second, one step per display frame, until the Current location starts
with `Leviticus`, then shows and writes the result to `Documents/s07-bench.txt` in the app's
container.

## Evidence

See the measurements table below; filled in from the runs listed there.
