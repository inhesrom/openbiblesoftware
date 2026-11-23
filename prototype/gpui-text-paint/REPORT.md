# Painting our Resource-text pipeline through GPUI (#372): prototype report

> **PROTOTYPE, throwaway.** This note is on the branch `prototype/372-gpui-text-paint` only. It
> answers #372 on the GPUI map #371: how does the GPUI UI draw Resource text that `obs-layout`
> has already shaped, ordered and broken into lines (ADR 0004), within the #19 targets?

## Answer: approach (a), `Window::paint_glyph` with our glyph ids

**Recommended: approach (a).**
- Register the reading fonts' files with GPUI's text system, and map each `obs_fonts::FaceId`
  to a GPUI `FontId` once.
- Paint each glyph that `obs-layout` placed with `Window::paint_glyph(origin, font_id,
  GlyphId(id), size, colour)`.
- GPUI rasters each glyph once, at its subpixel variant and the window's scale factor, and
  keeps it in its own sprite atlas. Our `obs-render` glyph atlas is not used on GPUI.

The evidence:
- **Correct text.** The prototype draws the four Bibles as the egui app does:
  - Hebrew wraps right to left, with points and cantillation;
  - Greek breaks at spaces;
  - "Lord" is in small capitals;
  - verse and chapter numbers are placed as in egui.

  It was checked against the egui app's screenshots at the same moments (below).
- **Same glyph ids.** At startup the prototype shapes sample text with our harfrust shaping and
  with GPUI's own text system, using the same font files. The glyph ids are identical for
  Gentium Plus Regular, Gentium Plus Italic, Greek in Gentium Plus, and Ezra SIL Hebrew with
  points and cantillation: 39/39, 16/16, 18/18 and 26/26 glyphs. They are recorded in the bench
  JSON under `glyph_id_check`. So the glyph we shape is the glyph GPUI rasters.
- **No new painter to own.** Glyph rasterization, caching, subpixel variants, scale changes and
  atlas growth are GPUI's, and they are the same code Zed uses for all its text. The #255
  atlas-lifecycle work in `obs-render` is not needed on GPUI.
- **Cost.** About 2,800 `paint_glyph` calls per frame for four full Panels. On Xvfb with CPU
  rendering, the UI thread's frame work was 2.2 ms at p50 and 3.4 ms at p99, including the
  painting.

**Approach (b), drawing `obs-render`'s quads through GPUI, is not practical.** GPUI 0.3.12 has
no public way to draw a tinted sub-rectangle of a texture the app owns. Here is what it offers:
- `Window::paint_image` draws a `RenderImage`: immutable BGRA frames that GPUI uploads into its
  own atlas, keyed by the image's id. It can show a sub-rectangle, but as a polychrome sprite
  with no tint colour, only a grayscale flag.
- Our atlas is a single-channel coverage texture that grows and starts over (#255). Each change
  would need a new `RenderImage`: up to 2048² × 4 bytes re-uploaded, with colours baked into the
  pixels (one copy per colour role).
- `paint_surface` is macOS-only (a `CVPixelBuffer`).
- The sprite types (`MonochromeSprite`, `PolychromeSprite`) are public. But the frame's scene
  they go into (`Window::next_frame`) and GPUI's sprite atlas are crate-private. Only
  `paint_glyph`, `paint_svg` and `paint_image` add sprites.

So (b) would mean patching GPUI. Approach (a) needs none of that.

## What the prototype does

[`prototype/gpui-text-paint`](.):
- **`src/data.rs`** reads each Bible as `ReadingDoc` does: reading units, `unit_blocks`, Verses
  and the Verse map. Paragraphs come from `block_paragraph` with the default `TextSettings`.
  Height estimates are made for every block off the UI thread (`HeightEstimate`), as the egui
  app's `HeightIndex` does.
- **`src/layout.rs`** lays text out off the UI thread:
  - 2–4 worker threads run `obs_layout::layout` (shared `ShapeCache`) into a cache keyed by
    (Bible, block, width in quarter points);
  - the UI thread lays a block out only within a 2 ms budget per frame (`SYNC_BUDGET`, as in
    `crates/ui-egui/src/view.rs`), or for the block a jump lands in, which is exempt;
  - the workers prefetch ±1.5 viewports.
- **`src/para.rs`** has `ParaElement`, one GPUI element per laid-out block in view:
  - it paints only the lines in view, with `paint_glyph`;
  - it exposes `Role::Paragraph`, with the block's text as its value and a right-to-left text
    direction for Hebrew;
  - its Tagged words and Scripture references (up to 100) are synthetic `Role::Link` children,
    each with its label and bounds.
- **`src/main.rs`** has the shell, linking and bench driver of `prototype/gpui-shell`:
  - each Panel's scroll model is (block, offset), as in the egui app's `ReadingView`, so a block
    laid out late changes no position;
  - jumps put the verse's line at the top.

## Correctness evidence (Xvfb `:97`, 1×)

Taken by `bench.sh` at the same scripted moments in both apps:

| | GPUI, our pipeline | egui app |
|---|---|---|
| Genesis 1:1 | [gpui-genesis-1.png](screenshots/gpui-genesis-1.png) | [egui-genesis-1.png](screenshots/egui-genesis-1.png) |
| John 1:1 (TAHOT at Exodus 20) | [gpui-john-1.png](screenshots/gpui-john-1.png) | [egui-john-1.png](screenshots/egui-john-1.png) |

Close-ups:
- [Hebrew](screenshots/zoom-hebrew-gpui-left-egui-right.png): Genesis 1, GPUI left, egui right.
  The lines break at the same words and wrap right to left. The marks sit the same.
- [Greek](screenshots/zoom-greek-gpui-left-egui-right.png): John 1. It breaks only at spaces,
  unlike GPUI's own text in `prototype/gpui-shell`.
- [English](screenshots/zoom-latin-gpui-top-egui-bottom.png): Genesis 1, GPUI on top.
- [Genesis 2:4](screenshots/gpui-genesis-2-4-small-caps.png): "Lord" in small capitals in the
  KJV; italics; the footnote letters.

Differences seen:
- **Line breaks.** Some English and Greek lines break at other words, because the column is a
  little wider than egui's. The prototype's column width is the Panel minus a fixed 24-point
  inset; egui uses its Reading layout column and insets. The layout itself is identical.
- **Anti-aliasing.** GPUI's glyphs show slight colour fringes (subpixel anti-aliasing, which
  `paint_glyph` picks for an opaque window when the platform supports it). egui's are grayscale.
  Turning it off is one call, if wanted:
  `App::set_text_rendering_mode(TextRenderingMode::Grayscale)`.
- **Not drawn:** egui's large chapter numerals; the header's second row; Note markers; the jump
  flash.

## Xvfb smoke run: not representative

`DISPLAY=:97 SKIP_BUILD=1 RUNS=1 prototype/gpui-text-paint/bench.sh`. Both apps ran with Mesa
llvmpipe (CPU rendering), while two other agents' builds and apps shared the machine and the
display. Raw results: [results/xvfb-smoke/](results/xvfb-smoke/).

| | GPUI, our pipeline | egui app |
|---|---:|---:|
| UI-thread frame work, all phases: p50 / p95 / p99 / max (ms) | 2.23 / 2.95 / 3.40 / 16.8 | 1.61 / 2.52 / 4.93 / 48.4 |
| Steady scroll: p50 / p99 (ms) | 2.17 / 3.16 | 1.47 / 3.35 |
| Jumps: p50 / p99 / max (ms) | 2.37 / 9.14 / 16.8 | 2.06 / 8.69 / 48.4 |
| Frames that showed a block not yet laid out | 0 of 1,533 | – |
| UI-thread layout per frame: p99 / max (ms) | 0.002 / 14.5 ¹ | – |
| Blocks laid out: on the UI thread / by workers | 96 / 376 | – |
| Glyphs painted per frame, p50 | 2,765 | – |
| Shape cache hit rate | 0.85 | – |
| Peak RSS (MB) | 850 | 841 |

¹ The maximum is a jump's own block, which is laid out at once (TAHOT's Psalm 119 is one
21.7 KB paragraph). That is the same exemption the egui app makes.

Frame intervals are not comparable here. GPUI paced itself at 48 ms under Xvfb on the loaded
machine, while eframe ran unpaced. The prototype in `prototype/gpui-shell` (GPUI 0.2.2, its own
text, shaped on the UI thread) had jump hitches of 50–125 ms. With layout off the UI thread they
are gone: the worst frame here was 16.8 ms, from the exempt jump block.

## For the real numbers (owner's desktop, 120 Hz)

In the desktop session:

```sh
cd ~/.paseo/worktrees/1svrye1w/proto-372   # or a checkout of prototype/372-gpui-text-paint
RUNS=3 prototype/gpui-text-paint/bench.sh
```

On Hyprland it reads the refresh rate by itself. Otherwise prefix `OBS_PROTO_HZ=120`. Keep the
benchmark windows on screen until it finishes. Results go to
`prototype/gpui-text-paint/bench-out/<date>-<host>/summary.md`.

## Accessibility

- **The tree.** Each block in view is a GPUI element with an id and `Role::Paragraph`.
  `write_a11y_info` sets the block's text as the value, and a right-to-left text direction for
  Hebrew. `a11y_synthetic_children` adds a `Role::Link` leaf for each Tagged word and Scripture
  reference: its label is the word's text, and its bounds are its first line's box.
- **Bounds.** Synthetic nodes need bounds relative to the window's content, in device pixels
  (points × scale factor), with no transform. Element nodes are built the same way in GPUI:
  `gpui-pre` 0.3.12 `src/element.rs` sets each element node's bounds from its window-relative
  layout bounds × `window.scale_factor()`, and no node gets a transform. That matches the
  `accesskit::Rect` convention for a tree without transforms.
- **Tree building is lazy.** GPUI builds the tree only while the platform adapter reports an
  assistive technology (`Window::is_a11y_active`). Synthetic children are only asked for then.
- **Untested.** No screen reader is available here, and Xvfb has no AT-SPI client. One attempt
  turned AT-SPI on in a private D-Bus session, but GPUI never reported an active assistive
  technology, so no tree was dumped. To inspect the tree on the desktop with Orca running, set
  `OBS_PROTO_A11Y_DUMP=/tmp/tree.json`, or use Accerciser.

## Open questions

- **The desktop numbers.** Do they hold steady scrolling at 120 Hz with about 2,800
  `paint_glyph` calls per frame? GPUI raster-caches each glyph, but each call still looks the
  glyph up. If needed, paint only changed blocks, or batch glyphs.
- **Anti-aliasing.** Should GPUI's subpixel rendering stay on for Resource text, given that the
  egui look is grayscale?
- **Screen readers.** Are the synthetic links' bounds right, and is a paragraph's value read
  properly? This needs VoiceOver, NVDA and Orca, and belongs with #380 and #398.
- **The text model.** The text a screen reader gets is the laid-out text, with verse numbers in
  it. Should it be the unit's text instead? That is the same question the egui app answered in
  `a11y.rs`.
- **Variable fonts and user fonts (#138).** GPUI has to load the same face from the same bytes.
  With `FaceId::User(n)`, register the user's font bytes with `add_fonts` and resolve them by
  family and style. Not tried.

## Draft amendment for ADR 0004 (not applied)

```md
## Amendment (#372): on GPUI, our laid-out glyphs are painted with GPUI's glyph painter

When the desktop UI moves to GPUI (ADR 0025), Resource text keeps this pipeline for shaping
(harfrust), direction (unicode-bidi), line breaking and hit-testing; only the painter changes.
The reading fonts' files (the Bundled fonts and loaded user faces, #138) are registered with
GPUI's text system, each `FaceId` is mapped once to a GPUI `FontId`, and every glyph a laid
block places is painted with `Window::paint_glyph` at our position. The glyph ids are the same
because the font files are: the prototype checked Gentium Plus regular, italic and Greek and
Ezra SIL Hebrew with points and cantillation (#372). GPUI rasters each glyph at its subpixel
variant and scale and keeps it in its own atlas, so `obs-render`'s glyph atlas and its
lifecycle (#255) are not used on GPUI. Drawing our atlas through GPUI was rejected: GPUI has no
public way to draw a tinted part of an app-owned texture, and re-uploading the atlas as an image
on every change, with colours baked in, would cost more than it saves.

Layout stays off the UI thread as the #19 amendment says: workers lay blocks out into a cache
keyed by Resource, block and width; the UI thread lays out at most `SYNC_BUDGET` (2 ms) per
frame, and the block a jump lands in at once. Each laid block in view is one GPUI element with
`Role::Paragraph` and its text; its links (Tagged words, references) are synthetic children
with bounds relative to the window, in device pixels, as GPUI gives its own nodes.
```
