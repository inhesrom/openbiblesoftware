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
- **Atlas lifecycle (#255):** a block's glyphs are drawn with the atlas texture as it is right after they are placed. Growing keeps every glyph where it was; when even the largest atlas (2048²) is full, it starts over and the rest of the block is drawn as a further mesh with the new texture, while earlier meshes keep the texture they sample until it has been painted. All the textures a frame paints with stay within the 32 MiB text GPU budget of #19: a replaced texture is emptied and filled again once its pass is painted, and a start-over that would pass the budget (one in the frame that grew the atlas, or a third texture) leaves the rest of the glyphs to the next frame. When the glyphs in view need more than the budget's two full atlases, that next frame and those after raster them at half the device scale each time (down to an eighth) until every glyph in view is drawn; once the last frame's glyphs would take a quarter of an atlas or less at the finer scale, it goes back to it. Only lines in view are placed, so a screenful seldom needs a second mesh, let alone a coarser one.
- **Accessibility:** every laid-out block emits an AccessKit paragraph node (text and bounds), with Tagged words and references as link children. Only visible and prefetched blocks emit nodes.

## Amendment (#138): Reading fonts and the Interface font are the user's choice

Resource text no longer uses only the Bundled fonts. View → Fonts chooses a Reading font for each script (Latin, Greek, Hebrew) and an Interface font, each either a Bundled font or a font installed on the computer. So:

- **Installed fonts are read by the app, not the libraries.** The desktop app enumerates them with `fontdb` and hands their bytes to `obs-fonts` (`load_user_face`), through the `SystemFonts` trait of `obs-ui-egui`, as Get Bibles does with its transport. `obs-fonts`, `obs-layout` and `obs-engine` stay free of platform font code and still build for WASM, where only the Bundled fonts are offered.
- **Loaded faces are `FaceId::User(n)`, never reused.** Shaped segments and atlas glyphs are keyed by face, so a stale entry can never be drawn in another font. Changing a font loads new faces, unloads the old ones and clears the shaped-segment cache; the Text settings change, so every view lays its text out again (anchored on its verse, so Annotations and selection stay put).
- **Fallback is per grapheme cluster, decided in layout.** A cluster the chosen face cannot draw (by its cmap) is drawn in the Bundled font for its script, so text never shows missing glyphs. A font with no `smcp` gets synthetic small capitals for the divine name.
- **Choices are stored by family and style name** in the User database, not as files. A font missing on another computer falls back to the Bundled font, with a notice once; the choice is kept for computers that have it.
- **The Interface font goes first in egui's proportional family**; egui's own fonts stay behind it, so emoji and symbols still draw. Headings keep Gentium Plus Bold.

Considered: a font per Resource (left for later), and `font-kit` instead of `fontdb` (native system APIs, but heavier and with C dependencies on Linux).
