# Mobile text view prototype (S07, #331)

The prototype that answers #331 for iOS: Resource text drawn by our own pipeline (ADR 0004)
in a hosted UIKit view, with native scrolling, selection, VoiceOver and Dynamic Type. Plan
`docs/plans/mobile-ios.md` A4 (option B, CPU tiles) and S07. It is throwaway-quality on
purpose: the Rust surface is marked `S07 prototype` and S13 replaces it; the Swift is rough.
What it must not be is a place where study rules live in Swift (ADR 0021 rule 3): every
reading-text colour, word or Verse decision, Verse label, reference and count the screen
shows comes from Rust. The few things Swift still decides are listed under "Decided in
Swift" below, accepted for the prototype.

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
  `go_to_chapter(reader, book, chapter)` goes to one (the grid stub). `open_reader` of a
  Resource already open gives back its reader where it was, so switching Bibles back and
  forth opens each once.
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

**Decided in Swift (accepted prototype gaps; the Shell audit lists them too):**

- **Word snapping while a handle is dragged.** `CoreTokenizer` overrides only
  `rangeEnclosingPosition`, which the core's `enclosing_span` answers.
  `isPosition(_:atBoundary:inDirection:)`, `position(from:toBoundary:inDirection:)` and
  `isPosition(_:withinTextUnit:inDirection:)` fall back to UIKit's
  `UITextInputStringTokenizer`, so the snapping to word ends during a drag is UIKit's, over
  `text(in:)`. S14a.
- **Text direction.** `position(from:in:offset:)` maps left and up to a negative (earlier)
  offset whatever the text's direction, and `baseWritingDirection` and
  `TextSelectionBox.writingDirection` answer `.natural`. In the TAHOT (right to left),
  visual left is logically forward, so these answers are wrong there; the core's
  `doc.direction()` is not asked. S14a.
- **Labels and chrome colours.** The sheet's section and field labels ("Word", "Strong's",
  "Lemma", "Gloss", "Verse"), "No Bible in the library", "Starting…", "Could not start",
  the reference field's placeholder, the menu's "Bible", "Page tone", "Show measurements"
  and "About", "Books", and the accessibility labels "Books and chapters" and "Reading
  options" are Swift strings; the reference error is `.red` and the reference field's
  background `.secondarySystemBackground`. The page and ink colours are the core's.

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

Measured on 2026-11-21 on the owner's Mac (Apple M6, 16 GB, macOS 27.0.1, Xcode 27.2), iPhone 17
simulator (iOS 27.2, 3×, 60 Hz display link), the BSB from the Bundled set. Simulator numbers
are indicative only; S08 judges on the device.

| Go criterion (plan S07) | Simulator result | Met |
|---|---|---|
| Fling through Genesis–Exodus with ≤ 1 % dropped frames | 0 % at 8000 pt/s (2828 frames, 47 s) and at 16000 pt/s (1414 frames, 23 s), Release; 0 % at 8000 pt/s in Debug | yes |
| A tile composites in ≤ 4 ms at 3× | Rust's time per tile (one band, up to 1170 × 768 px): p50 0.17–0.3 ms, p95 0.45–1.1 ms, max 0.65–5.4 ms over about 2780 tiles a run | yes (p95) |
| Cold start to first text ≤ 1.5 s | Release 1.53–1.95 s over 8 launches (1.53, 1.60, 1.61, 1.64, 1.66, 1.67, 1.75, 1.95); Debug about 3.0 s | **no** |
| Selection handles, loupe and the system edit menu on long-press | the long-press selects the word with both handles and the edit menu's Copy (`testPrototypeSmoke`, `s07-selection.png`); the loupe shows only while a handle is dragged, which XCUITest cannot hold, so S08 checks it | yes (loupe: S08) |
| One accessibility element per Verse, labelled with its reference and text, in reading order; `performAccessibilityAudit()` passes | `testPrototypeSmoke` (Genesis 1:1… in order, one each) and `testAccessibilityAudit` pass | yes |
| Typed "Jn 3:16" navigates | `testPrototypeSmoke` | yes |
| A tap on a word opens the stub sheet with the Verse reference and its cross-reference count; with the Study set override, a Strong's number | "John 3:16 (BSB)", "23 cross-references" (`s07-sheet.png`); KJV with Strong's: "loved", G25, ἀγαπάω (`testATaggedWordShowsItsStrongsNumber`, `s07-sheet-strongs.png`) | yes |
| RTL Hebrew paragraph renders right-aligned | the TAHOT, Genesis 1 (`s07-hebrew.png`) | yes |

More measurements:

- **The faster fling.** At 30000 pt/s (500 pt a frame, beyond what a finger gives) 33 % of
  frames drop: the tile queue falls behind and tiles in view are drawn on the main thread.
- **Swift's side of a tile:** 3.6–5 ms p50, 8–12 ms p95, timed as one span (the FFI call,
  the image and its preparation for display), the same in Debug and Release. The split
  between them is unmeasured. What is confirmed is that the Rust side lowers the pixels one
  byte at a time: UniFFI 0.32's `uniffi_core` `Lower for Vec<T>` writes each item in a loop,
  about 3.5 MB a tile. Before the tile queue took the tiles drawn ahead off the main thread,
  8000 pt/s dropped 3.0 % (Release) and 4.2 % (Debug) of frames.
- **A frame:** `reader_frame` 26–65 µs p50, under 0.2 ms p95, on the main thread.
- **Cold start, in parts (Release):** process start → SwiftUI's `start()` 0.22–0.48 s; `boot`
  about 1.25–1.4 s; first tiles 60–90 ms later. On the host (release), `boot` is 0.73 s: opening
  the whole BSB (`ReadingDoc::open`) about 0.45 s and the reading view's height estimate of
  every block (`ReadingView::new`) about 0.28 s; the rest under 60 ms.
- **Memory footprint:** 72–76 MB after launch, 99–109 MB after the Genesis–Exodus fling.
- **Size:** `libobs_ffi.a` 27.4 MB (simulator) and 27.4 MB (device), against the 40 MB stop
  line. The Release device app (`OpenBible.app`, unsigned) is 59 MB, of which 43.1 MB is the
  Bundled set and 18.9 MB the executable.
- **Cost:** about 1,000 lines of Swift for the prototype (`ResourceTextView.swift` 381,
  `TextCanvasView.swift` 283, `ReaderModel.swift` 253, `ReaderScreen.swift` 171; the S06 hello
  screen is the rest), 107 lines of UI tests; 1,275 lines of Rust in `text_view.rs` (half of
  it tests) and the FFI mirror.
- **Option A (Metal in Swift)** was not built: B met the frame budget.

The bench in Release, from the repository root, after `cargo xtask ios build --sim-only` and
`xcodegen generate` in `apps/ios`:

```sh
cd apps/ios && xcodebuild build -scheme OpenBible -configuration Release \
  -destination 'platform=iOS Simulator,name=iPhone 17' -derivedDataPath ../../target/ios/xcode
SIMCTL=/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl
$SIMCTL boot 'iPhone 17'
$SIMCTL install booted ../../target/ios/xcode/Build/Products/Release-iphonesimulator/OpenBible.app
$SIMCTL launch booted io.github.inhesrom.openbible.dev -OBSBenchUntil Leviticus -OBSBenchSpeed 16000
# about a minute later:
cat "$($SIMCTL get_app_container booted io.github.inhesrom.openbible.dev data)/Documents/s07-bench.txt"
$SIMCTL shutdown all
```

The UI tests (`OBS_STUDY_SET_DIR` makes the Strong's test run instead of skipping):

```sh
cd apps/ios && TEST_RUNNER_OBS_STUDY_SET_DIR="$PWD/../../dist/starter" xcodebuild test \
  -scheme OpenBible -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath ../../target/ios/xcode
```

Previews (`docs/previews/ios/`): `s07-light.png` (Paper, Genesis 1), `s07-dark.png` (the
simulator in dark appearance, Auto → Dark), `s07-sepia.png` (`-OBSPageTone sepia`),
`s07-hebrew.png` (the TAHOT through the Study set override), `s07-selection.png`,
`s07-sheet.png`, `s07-sheet-strongs.png`.

## Known rough edges (prototype)

- A selection handle at the very top of the text view sits under the header and is cut off.
- Opening another Bible the first time reads the whole Resource on the main thread (about a
  second for a Bible), so the screen stops meanwhile.
- After blocks above the view are laid out at a new height, the selection's rectangles are a
  few points off until the selection changes; the selection is not redrawn on scroll
  corrections.
- No rotor "Verses" yet, and no accessibility scroll; VoiceOver moves through the Verses laid
  out around the view.
- The Page tone is kept for the run only.

## S08: the owner's test on a real iPhone

**Before** (once; S05 left these for the owner):

1. **Apple ID in Xcode.** Open Xcode → Settings (⌘,) → Accounts → the **+** at the bottom
   left → Apple ID → sign in. A free "Personal Team" appears under your name. Do not enrol in
   the paid Developer Program (S33 decides that).
2. **Phone and Mac trust each other.** Plug the iPhone into the Mac with a cable and unlock it.
   When the phone asks "Trust This Computer?", tap **Trust** and enter the phone's passcode.
3. **Xcode sees the phone.** From the repository root run `cargo xtask ios bundled-set` (once),
   then `cd apps/ios && xcodegen generate && open OpenBible.xcodeproj`. Keep the phone
   plugged in and unlocked; Xcode's top bar may say it is "preparing" the phone for a few
   minutes the first time.
4. **Developer Mode on the phone.** Now that Xcode has seen it: on the phone, Settings →
   Privacy & Security → Developer Mode (at the bottom) → turn it **On** → **Restart**. After
   the restart, unlock the phone and confirm **Turn On** and your passcode.
5. **Keychain prompt.** The first time Xcode signs the app (step 2 below), the Mac may ask
   "codesign wants to access key … in your keychain". Type your Mac login password and click
   **Always Allow** (it may ask more than once; answer the same each time).
6. **Seven-day limit.** An app signed by a free Personal Team stops opening after 7 days. If
   it will not open later, plug the phone in and press Run in Xcode again.

Then, from the repository root:

1. `cargo xtask ios build` (both slices; about 2 min the first time), then
   `cd apps/ios && xcodegen generate && open OpenBible.xcodeproj` if it is not open.
2. In Xcode: the `OpenBible` target → Signing & Capabilities → Team: your Personal Team. Keep
   the bundle id `io.github.inhesrom.openbible.dev`. Choose the iPhone as the run destination
   and press Run (the scheme runs **Release**, so this is the build that is measured). If the
   build stops with "the Rust core changed after Xcode had read it; build again", press Run
   again. The first time, the phone refuses to open the app ("Untrusted Developer"): on the
   phone, Settings → General → VPN & Device Management → your Apple ID → **Trust**, then
   press Run again.
3. **Cold start (required: the one go criterion the simulator missed, 1.53–1.95 s against
   1.5 s).** Force-quit the app (swipe up from the bottom edge and hold, then swipe the app's
   card up). Open it from the Home Screen and judge the time from your tap to the Bible text
   appearing: does it feel instant, or do you wait? Do it three times. Then run step 9 once
   and write down the "cold start N ms" it shows (that launch comes from Xcode, with its
   debugger attached, so it may read a little slower than a Home Screen launch).
4. **Feel.** Fling through Genesis into Exodus, fast and slow; flick to stop; rubber-band at
   the top. Note any blank band, hitch or jump of the text. In the Aa menu (Reading options),
   turn on **Show measurements**: after a fling the strip shows frames dropped and tile times.
5. **Input.** Long-press a word: it is selected, with handles; drag a handle (the loupe);
   Copy and paste the result into Notes (it ends with its reference, such as "Genesis 1:1
   (BSB)"). Tap the text to clear the selection. Type `Jn 3:16` in the field and press Go;
   try `Ps 23`, `Rev 22:21`, a typo. Open the grid button, pick a book and chapter. If you
   have the Hebrew keyboard (Settings → General → Keyboard → Keyboards → Add New Keyboard →
   Hebrew), switch to it in the field with the globe key and type a few letters: they should
   run right to left; Go then shows the parser's error (there are no Hebrew book names yet).
6. **Study sheet stub.** Tap a word: the sheet names the Verse and its cross-reference count.
   Does it come up as soon as you lift your finger (tap latency)? (No Strong's number on the
   phone: the Study set is not on it until S24's downloads; the simulator showed it through
   the Debug-only override.)
7. **Accessibility.** Turn on VoiceOver: swipe through Genesis 1; each Verse is read as its
   reference then its text, headings as headings. Larger Text: Settings → Accessibility →
   Display & Text Size → Larger Text → turn on **Larger Accessibility Sizes** first (without
   it the slider stops short), then drag the slider to the far right: the text grows (40 pt)
   and stays on the same Verse. Dark Mode: the page goes dark. Menu → Page tone: Paper,
   Sepia, Dark.
8. **Performance.** Read for ten minutes, scrolling now and then: is the phone warm?
9. **Fling bench (run once, for step 3).** In Xcode, Product → Scheme → Edit Scheme → Run →
   Arguments, add `-OBSBenchUntil Leviticus` and `-OBSBenchSpeed 8000`, Run: the app scrolls
   Genesis and Exodus on its own and shows the result at the bottom (frames dropped, tile
   times, "cold start N ms", footprint); read it off the screen. Remove the arguments
   afterwards.
10. **iPad (if you have one).** Repeat the Before steps 2 and 4 and steps 2–9 with the iPad as
    the run destination.
11. **Cost.** Read the Evidence above (the measurements, the Swift line count).

Not judged on the phone: **right-to-left Hebrew text**. The only Hebrew Resource (the TAHOT)
is in the Study set, which a phone gets only with S24's downloads; the simulator showed it
right-aligned through the Debug override (`s07-hebrew.png`).

Write the verdict (go or no-go, B or A, the Android half of #331, and performance notes with
step 3's cold-start judgement and number) as a comment on the S07 PR or a note the S09
executor can quote.
