# egui long-text rendering risk (issue #7)

**Question:** Can egui 0.37 (as forked by ArtCraft at `storytold/egui`) comfortably render scripture and commentary: long virtualized texts, cross-paragraph selection, clickable inline spans (footnotes, cross-refs, Strong's words), styled runs, complex shaping (Hebrew with niqqud/cantillation, polytonic Greek), and font fallback? How does `storytold/wordcraft` render long documents?

**Date:** 2026 (sources current to 2026-10-23, the newest dated item read). Fork state checked at `storytold/egui` default branch HEAD plus the rev WordCraft pins (`6d518fc…`).

## TL;DR

- **English and Greek: yes, with custom work.** egui 0.35+ shapes text with `harfrust`, a pure-Rust HarfBuzz port. It applies GPOS x/y offsets, so polytonic Greek combining marks should position correctly within LTR runs. Not tested by us.
- **Hebrew (RTL): no, not in stock egui 0.37 or in the storytold fork as read.** epaint has no bidi. RTL runs give 2n−1 glyphs. Words come out mirrored. Carets and selection break, and Ctrl+Left can panic. Upstream fix #8577 is an **open draft**. ArtCraft's own tracking issue lists bidi as planned toolkit work, not done.
- **Virtualization:** `ScrollArea::show_rows` handles fixed-height rows only. Variable-height verses or paragraphs need `show_viewport` plus your own height index, or the third-party `egui_virtual_list` crate.
- **Cross-label selection exists but is fragile.** Per its own PR, the selection is lost if you scroll past an endpoint, and drag-to-scroll selection does not work. That clashes with virtualization, because off-screen labels are never laid out.
- **Inline hit-testing is DIY.** `Label::layout_in_ui` returns the `Galley`. `Galley::cursor_from_pos` maps a point to a char index, which you map to your own span table. egui has no built-in link span inside a `LayoutJob`.
- **Fallback:** fonts are tried in order per family. 0.37 adds runtime system-font providers. Row height comes from the *first* font only, so stacked marks can be clipped.
- **WordCraft does not use egui text for documents.** It has its own pipeline: `wordcraft-fonts` (harfrust + skrifa), `wordcraft-layout` (unicode-bidi, line breaking, hit testing), and `wordcraft-render` (vello_cpu raster to cached page textures/tiles). egui only paints the caret, selection and chrome on top.
- **Main mitigation options:** (A) stay on egui text, English/Greek only in v0.1, and adopt the bidi patch later; (B) build a custom reader widget on parley (or a WordCraft-style pipeline) that paints into egui; (C) a hybrid: egui text for UI chrome, custom text for the reading panes.

## 1. Virtualized scrolling

| API | Fit for Bible/commentary | Evidence |
|---|---|---|
| `ScrollArea::show_rows(ui, row_height, total_rows, …)` | One uniform `row_height` only. Works for a one-line-per-verse list, **not** for wrapped verses or commentary paragraphs. | [docs.rs ScrollArea 0.37](https://docs.rs/egui/0.37.0/egui/containers/scroll_area/struct.ScrollArea.html) |
| `ScrollArea::show_viewport(ui, \|ui, viewport\| …)` | Gives the visible rect. You set the total height and lay out only the visible items. This is the supported route for variable heights. | same; [emilk/egui#1376](https://github.com/emilk/egui/issues/1376) |
| `egui_virtual_list` (third-party, MIT, by egui maintainer lucasmerlin) | Variable heights, measured lazily and cached. Items can be added at the top without the view jumping. | [docs.rs egui_virtual_list](https://docs.rs/egui_virtual_list/latest/egui_virtual_list/), [crates.io](https://crates.io/crates/egui_virtual_list) |

- emilk on variable heights (#1376, still **open** since 2022): "The only way to support this is if you know the heights of the elements beforehand… or you lay out your widgets once and then store the bounds… use `ScrollArea::show_viewport` and do a binary search." [#1376](https://github.com/emilk/egui/issues/1376)
- Meaning for us: the engine (or the UI crate) needs a **per-block height cache keyed by (width, font settings)**. Without it, the scrollbar for a 1,189-chapter Bible is either an estimate or needs full layout up front. egui's galley cache (which 0.37 stops evicting across viewports, [CHANGELOG #8528](https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md)) speeds up re-layout but is not a height index.

## 2. Selection across labels and paragraphs

- Cross-widget selection landed in [PR #3870](https://github.com/emilk/egui/pull/3870), which closed [#3816](https://github.com/emilk/egui/issues/3816). It is on by default and toggled with `style.interaction.multi_widget_text_select` ([docs.rs Interaction](https://docs.rs/egui/latest/egui/style/struct.Interaction.html)). The docs themselves say: "text selection can be slightly glitchy, so you may want to disable it."
- **Limitations listed by the author:** "Drag-select to scroll doesn't work"; "A selection disappears if you scroll past one of its end-points"; "Only the text of labels and links are selectable" ([#3870](https://github.com/emilk/egui/pull/3870)). The PR's TODO list ticks "Scrolling past selection behaves weird" as addressed, but the limitations section was not updated. **Unverified** whether 0.37 still drops the selection on scroll.
- How it works (fork source): the state stores only a `widget_id` + `CCursor` + last screen position for each end. Copy text is gathered from labels painted in the current pass, joined with `\n` or a space based on their screen rects ([label_text_selection.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/egui/src/text_selection/label_text_selection.rs)). **Consequence:** with virtualization, labels outside the viewport are never visited, so you cannot select or copy text that is off screen (e.g. Gen 1:1 through 3:24 in one drag). This follows from the code above; we did not test it.
- 0.37 adds double/triple-click-and-drag word/line selection ([#8370 in CHANGELOG](https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md)).
- Selection is limited to one layer, so it cannot span parallel-translation panes in different windows (`CurrentSelection.layer_id`, [source](https://raw.githubusercontent.com/storytold/egui/main/crates/egui/src/text_selection/label_text_selection.rs)).

## 3. Inline hit-testing (footnotes, cross-refs, Strong's, hover popups)

- `Label` has one `Sense` for the whole label. There is no per-section link or hover API ([label.rs in fork](https://raw.githubusercontent.com/storytold/egui/main/crates/egui/src/widgets/label.rs)).
- Building blocks that exist:
  - `Label::layout_in_ui(ui) -> (Pos2, Arc<Galley>, Response)`: lays out without painting ([label.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/egui/src/widgets/label.rs)).
  - `Galley::cursor_from_pos(Vec2) -> CCursor` and `pos_from_cursor(CCursor) -> Rect` ([text_layout_types.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/src/text/text_layout_types.rs)). `LayoutJob` sections are byte ranges, so char index → byte → section → our span id.
  - `Galley.job` keeps the original sections ([docs.rs Galley](https://docs.rs/epaint/0.37.0/epaint/text/struct.Galley.html)).
- Recommended upstream workaround for links: put separate `Label`/`Hyperlink` widgets in `horizontal_wrapped` with zero spacing ([discussion #4030](https://github.com/emilk/egui/discussions/4030)). This has a history of selection and link bugs in wrapped layouts ([PR #6905](https://github.com/emilk/egui/pull/6905)). It gives one widget per word, which is costly for Strong's-tagged text (roughly 790k tagged words in a Bible; our estimate).
- Open question: will a click on a span inside a *selectable* label fire `clicked()`? Selection senses `click_and_drag` on the same response ([label.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/egui/src/widgets/label.rs)), so click-versus-start-selection needs a prototype.
- `TextFormat` styling ([docs.rs](https://docs.rs/epaint/0.37.0/epaint/text/struct.TextFormat.html)): font, size, colour, background, italics, underline/strikethrough `Stroke`, letter spacing, line height, variation coords, `valign`. Superscript is approximated with a small font plus `valign = TOP`. There is no baseline-shift, small-caps (for LORD) or OpenType-feature field. Kerning and ligatures apply only *within* a section ([LayoutSection doc](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/src/text/text_layout_types.rs)). 0.36 adds `extra_text_line_spacing`; 0.35 adds `LayoutJob::format_at_byte` ([CHANGELOG](https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md)).

## 4. Complex shaping, RTL and fallback

| Topic | Status in egui 0.37 / storytold fork | Source |
|---|---|---|
| Shaper | `harfrust` (pure-Rust HarfBuzz port) since 0.35 ([#8031](https://github.com/emilk/egui/pull/8031)). Fork's epaint deps: `harfrust`, `skrifa`, `vello_cpu`, `unicode-segmentation`; **no `unicode-bidi`**. | [fork epaint/Cargo.toml](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/Cargo.toml) |
| Mark positioning | Applies shaper `x_offset` and `y_offset` per glyph. Polytonic Greek (LTR) should therefore place marks correctly when the font has GPOS mark data. **Not tested.** | [text_layout.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/src/text/text_layout.rs) |
| Run segmentation | By font face, not script. Source comment: "would need script-aware splitting once RTL/bidi support is added." | same |
| RTL/bidi | Not supported. [#1016](https://github.com/emilk/egui/issues/1016) is open since 2021. Since 0.35, RTL runs give more glyphs than chars, words render mirrored, carets are lost, and `Ctrl+Left` can underflow and panic ([#8576](https://github.com/emilk/egui/issues/8576)). | |
| Bidi fix | [#8577](https://github.com/emilk/egui/pull/8577) is an **open draft** (UAX #9 per paragraph, RTL shaping, visual reordering, direction-aware hit-testing). A tester reports Arabic harakat placed away from their letter, the wrong caret at an RTL line ending in LTR, and selection painting across a direction change. Arrow keys move logically. Smaller fix: #8698 (glyph count and underflow only, per #8576 thread). | |
| ArtCraft plan | [craft-libs#198](https://github.com/storytold/craft-libs/issues/198) (open) plans to carry #8577 in `storytold/egui`. Its 2026-10-21 progress comment says the work lives on `aiongg/egui craft/0.37`, "Nothing is merged". The fork's default branch has no bidi (above). | |
| Line breaking | Breaks at spaces, CJK, `-` and ASCII punctuation, else anywhere. No UAX #14. | [text_layout.rs `RowBreakCandidates`](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/src/text/text_layout.rs); [#198](https://github.com/storytold/craft-libs/issues/198) |
| Row height / fallback | "Row height comes from the first font only; fallback fonts are centred rather than baseline-aligned… Arabic marks and Vietnamese stacked accents lose pixels." This is a risk for Hebrew with cantillation and for stacked Greek diacritics. | [#198](https://github.com/storytold/craft-libs/issues/198) |
| Fallback chain | Per family, in order; then `GlyphRasterizer`s and `FontProvider`s (0.37: `egui_system_font_provider` via fontique on native; canvas glyph rasterizer on web). `default_fonts` is no longer a default feature. | [CHANGELOG 0.37](https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md); [text_layout.rs](https://raw.githubusercontent.com/storytold/egui/main/crates/epaint/src/text/text_layout.rs) |
| Upstream parley rewrite | [#5784](https://github.com/emilk/egui/pull/5784) is an open WIP draft. #198 says it "stalled" because egui's editing works on its own text type. | |

## 5. How WordCraft renders long documents

WordCraft is a word processor on egui 0.37 + the storytold fork ([Cargo.toml `[patch.crates-io]`](https://github.com/storytold/wordcraft/blob/main/Cargo.toml)). **It bypasses egui text for document content entirely:**

| Layer | Crate | Deps / job |
|---|---|---|
| Fonts and shaping | `wordcraft-fonts` | `skrifa 0.47`, `harfrust 0.13`, `unicode-bidi`, `unicode-script`. Its own font DB (bundled OFL + system) and explicit `harfrust::Direction` ([crates/fonts](https://github.com/storytold/wordcraft/tree/main/crates/fonts)). |
| Layout | `wordcraft-layout` | `unicode-bidi`, `unicode-linebreak`, `unicode-segmentation`. Modules `para` (shaping, line breaking, cached per paragraph revision in `LayoutCache`), `display` (draw list) and `hit` ("point ↔ position, caret geometry, line navigation, selection rectangles") ([crates/layout/src/lib.rs](https://github.com/storytold/wordcraft/blob/main/crates/layout/src/lib.rs)). |
| Raster | `wordcraft-render` | `vello_cpu` (multithreaded on native): "pages and thumbnails from the layout display list" ([crates/render/Cargo.toml](https://github.com/storytold/wordcraft/blob/main/crates/render/Cargo.toml)). |
| UI | `wordcraft-ui-egui` | `canvas.rs`: "pages… rendered into cached page textures (or visible full-resolution tiles for exceptionally tall pages), with the caret, selection, rulers and mouse editing drawn by egui on top." Keeps textures for pages within 3 viewport heights, with 1024-px tiles ([crates/ui-egui/src/canvas.rs](https://github.com/storytold/wordcraft/blob/main/crates/ui-egui/src/canvas.rs)). |

- README claims re-layout takes about 1.4 ms while typing in a 188-page document, thanks to per-paragraph caching ([README](https://github.com/storytold/wordcraft#why-wordcraft)). This is the maintainers' figure; we did not reproduce it.
- WordCraft is MIT OR Apache-2.0, so we can study or reuse its patterns or code with attribution. Its crates are word-processor shaped (pages, docx), so treat them as reference, not a drop-in dependency. **Unverified:** whether any of them are published on crates.io for reuse.
- Trade-off of the raster-to-texture approach: text is a bitmap. That means re-rendering on zoom or DPI change, no egui accessibility text unless added separately, and texture memory to manage. In exchange: full control of bidi, marks, fallback, hit-testing and selection, and the same pipeline can feed PDF/print.

## 6. Gaps and concrete mitigations

| Gap | Severity for us | Mitigation options |
|---|---|---|
| Hebrew RTL broken in egui text | **High** if the v0.1 scope includes original-language Hebrew. Low if v0.1 is English Bibles and Strong's shows only transliteration or lexicon glosses. | (a) Defer Hebrew display. (b) Carry #8577 + #8698 in our fork or track storytold's fork once it lands bidi. (c) Custom text widget with its own bidi (parley, or a WordCraft-style harfrust + unicode-bidi stack). |
| Variable-height virtualization | Medium | `show_viewport` plus an engine-side height index; or `egui_virtual_list`. Unit of virtualization = paragraph/pericope, not verse-per-widget. |
| Selection across virtualized/off-screen text | Medium | A custom selection model in document coordinates (verse ref + char offset), as WordCraft does in `layout::hit`. Copy from the engine model, not from painted labels. |
| Per-word hit-testing (Strong's, footnotes) | Medium | `layout_in_ui` + `cursor_from_pos` + a span table per galley (a cheap prototype). Or a custom widget with its own hit-test. |
| Stacked-mark clipping / fallback baseline | Medium (Greek/Hebrew) | Explicit `line_height`; bundle one font per script as primary family (OFL Greek/Hebrew fonts) rather than relying on fallback; or a custom layout. |
| No small caps / OT features in `TextFormat` | Low–Medium (LORD, verse numbers) | Simulated small caps (smaller uppercase section). A custom pipeline can set harfrust features. |
| WASM later | Low | harfrust, skrifa, vello_cpu and parley are pure Rust. WordCraft ships a wasm build with the same stack ([README](https://github.com/storytold/wordcraft#quick-start)). |

**Custom reader widget sketch (option C), so the cost is visible:** the engine holds a block model (verse/paragraph runs with span ids). `ui-egui` lays out visible blocks with parley (bidi, fallback via fontique, `Cursor`/`Selection` in `parley/src/editing/`; [parley repo](https://github.com/linebender/parley)) or harfrust directly. It then paints either (1) glyph quads through egui's painter/mesh with our own glyph atlas, or (2) vello_cpu tiles uploaded as egui textures, the WordCraft route. Hit-testing and selection run against our layout, and egui handles input, scrolling and popups. Cost: substantial, since accessibility, IME and selection painting become ours. Parley is Apache-2.0 OR MIT ([README](https://github.com/linebender/parley)), so licensing is compatible.

## 7. Implications for our decisions

- **Locked: "English Bibles, Strong's word study".** Most v0.1 reading works on stock egui text (option A). The real egui-text risks are: variable-height virtualization, selection that does not survive virtualization, and per-word hit-testing. All three need prototypes, not research.
- **Showing Hebrew text in any pane (lexicon headwords, interlinear) touches the RTL gap.** Decide early whether v0.1 shows Hebrew script. If it does, choose between carrying an unmerged draft patch (#8577, known defects) and a custom text path.
- **"Modeled on ArtCraft" does not mean "use egui text for documents."** The closest sibling, WordCraft, built its own shaping, layout, hit-testing and raster stack and treats egui as chrome. If we expect Hebrew/Greek, interlinear and rich commentary, the same split (engine-side layout, WASM-clean) fits our engine/ui-egui separation.
- **A fork dependency is a moving target.** WordCraft pins `storytold/egui` by commit. Its bidi work is planned (#198), not merged. Pin a rev and re-check before relying on it.
- **Suggested spikes, not decisions:** (1) `show_viewport` + height cache over full KJV, measuring frame time; (2) Strong's hover via `cursor_from_pos` inside a selectable `Label`; (3) Hebrew/Greek rendering test with chosen fonts on fork HEAD vs #8577; (4) a minimal parley → egui painter widget.

## Sources

- Kept: egui CHANGELOG (0.35–0.37: harfrust, font providers, selection drag) — primary release notes.
- Kept: emilk/egui #1016, #8576, #8577, #8031, #5784, #3870/#3816, #1376, #6905, discussion #4030 — primary issue/PR evidence.
- Kept: storytold/egui source (`epaint/Cargo.toml`, `text_layout.rs`, `text_layout_types.rs`, `label.rs`, `label_text_selection.rs`) — the actual fork code.
- Kept: storytold/wordcraft (`Cargo.toml`, README architecture, `crates/{fonts,layout,render,ui-egui}`) — primary evidence of the bypass architecture.
- Kept: storytold/craft-libs#198 — ArtCraft's own audit of egui text gaps and its fork plan.
- Kept: docs.rs ScrollArea/Galley/TextFormat 0.37; egui_virtual_list docs; linebender/parley README.
- Dropped: sparkles-docs page on show_rows (secondary); skyler.io mirror commits (duplicate of GitHub); filmcraft PR #1140 (app-specific workaround, covered by #198).

## Gaps

- Did not run code: Greek mark positioning, Hebrew niqqud/cantillation output, click-versus-selection in a selectable label, and selection after scroll in 0.37 are all **unverified**.
- Did not check whether the fork's pinned rev `6d518fc` differs from default-branch HEAD in text code. The commit itself is an IME change ([commit](https://github.com/storytold/egui/commit/6d518fc799681571963842f95b55a6b68431b15a)).
- Did not read the `wordcraft-layout` `hit`/`para` sources in depth, or measure the memory and latency of its texture approach.
- Did not open #8698 directly (cited via the #8576 thread).
