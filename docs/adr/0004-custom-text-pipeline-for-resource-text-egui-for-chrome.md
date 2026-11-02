# Resource text is laid out and drawn by our own pipeline; egui renders chrome only

All text that comes from a Resource goes through our own pipeline, wherever it appears: reading panes, Text Comparison cells, Guide sections, Word Info, search results, hover popups. The pipeline uses harfrust and skrifa for shaping, unicode-bidi for direction, unicode-linebreak for line breaking, our own hit-testing and selection, and a shared glyph atlas (glyphs rastered once, then drawn as egui meshes) for painting (amended below; originally vello_cpu block textures). This follows WordCraft's architecture (MIT OR Apache-2.0), whose code we adapt with attribution rather than depend on. egui's own text handles only UI chrome: menus, labels, buttons and text inputs. Stock egui and the storytold fork as of rev 6d518fc have no bidi, so they cannot show the full Hebrew OT that v0.1 requires, and egui's label selection does not survive virtualization.

## Considered Options

- **egui text plus the draft bidi patch (#8577):** least code, but the patch is unmerged and has known mark-placement and caret defects. Per-word hit-testing and selection across off-screen text would still be DIY.
- **parley:** has bidi, line breaking and selection built in, but brings in a second font system (fontique) alongside egui's harfrust and skrifa. No sibling app has proven it inside egui.
- **WordCraft-style harfrust + skrifa + unicode-bidi + vello_cpu (chosen for layout; painting amended below):** proven in the closest sibling app, shares the shaper egui already uses, is pure Rust and WASM-clean, and the same display list can later feed print or PDF.
- **Glyph atlas emitted as an egui mesh**, instead of vello_cpu block textures: crisper when zooming and lighter on memory, but we would own the atlas. Originally kept as a fallback; **now the chosen painter** (see the amendment).

## Consequences

- Text is drawn as bitmaps, so the pipeline must expose its own AccessKit nodes to be accessible. It must also re-raster on DPI or zoom changes and manage texture memory.
- Text inputs stay egui `TextEdit`, so typing Hebrew into them is unsupported in v0.1. Hebrew is reached by navigation, Strong's number, or "search this lemma" from Word Info. Revisit when the fork lands bidi.
- Layout, hit-testing and raster sit below the UI layer with no egui dependency.

## Amendment (#19): glyph atlas is the painter; layout runs off the UI thread

[Performance targets and texture strategy](https://github.com/inhesrom/openbiblesoftware/issues/19) measured both painters in the app shell prototype (`prototype/app-shell`, `OBS_BENCH`). The test scrolls 4 linked Panels at 120 Hz on a 2× display. Block textures were hardened with ≤1024 px tiles and a 4-thread vello_cpu worker pool, and still showed placeholders on 4–75% of frames and used 47–65 MB of texture. A Hebrew paragraph can be ~9.6k px tall at 2×. The glyph atlas showed no placeholders and used 16.8 MB in total, whatever the Panel count. Output looks the same by eye, including Hebrew marks. So:

- **Painting:** one shared glyph atlas (single-channel, subpixel-binned, grown by page) for all Resource text. Each laid-out block becomes an egui mesh. Block textures are dropped from v0.1. A later print or PDF path feeds the same laid-out lines to vello_cpu directly.
- **Layout off the frame:** a shaped-segment cache shared by all threads (90% hit rate on Genesis) and a layout worker pool that prefetches ±1.5 viewports. The UI thread may spend at most 2 ms per frame on synchronous layout. The block a jump lands in is exempt and is always laid out at once; linked followers and inline snippets settle off-thread a few frames later. With these, steady scrolling held 120 Hz (0.2–0.4% missed frames) without reading-pane placeholders.
- **Accessibility:** every laid-out block emits an AccessKit paragraph node (text and bounds), with Tagged words and references as link children. Only visible and prefetched blocks emit nodes.
