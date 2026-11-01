# Resource text is laid out and drawn by our own pipeline; egui renders chrome only

All text that comes from a Resource goes through our own pipeline, wherever it appears: reading panes, Text Comparison cells, Guide sections, Word Info, search results, hover popups. The pipeline uses harfrust and skrifa for shaping, unicode-bidi for direction, unicode-linebreak for line breaking, our own hit-testing and selection, and vello_cpu to raster cached block textures that egui then paints. This follows WordCraft's architecture (MIT OR Apache-2.0), whose code we adapt with attribution rather than depend on. egui's own text handles only UI chrome: menus, labels, buttons and text inputs. Stock egui and the storytold fork as of rev 6d518fc have no bidi, so they cannot show the full Hebrew OT that v0.1 requires, and egui's label selection does not survive virtualization.

## Considered Options

- **egui text plus the draft bidi patch (#8577):** least code, but the patch is unmerged and has known mark-placement and caret defects. Per-word hit-testing and selection across off-screen text would still be DIY.
- **parley:** has bidi, line breaking and selection built in, but brings in a second font system (fontique) alongside egui's harfrust and skrifa. No sibling app has proven it inside egui.
- **WordCraft-style harfrust + skrifa + unicode-bidi + vello_cpu (chosen):** proven in the closest sibling app, shares the shaper egui already uses, is pure Rust and WASM-clean, and the same display list can later feed print or PDF.
- **Glyph atlas emitted as an egui mesh**, instead of vello_cpu block textures: crisper when zooming and lighter on memory, but we would own the atlas. Kept as a fallback if texture memory or zoom quality fails the performance targets.

## Consequences

- Text is drawn as bitmaps, so the pipeline must expose its own AccessKit nodes to be accessible. It must also re-raster on DPI or zoom changes and manage texture memory.
- Text inputs stay egui `TextEdit`, so typing Hebrew into them is unsupported in v0.1. Hebrew is reached by navigation, Strong's number, or "search this lemma" from Word Info. Revisit when the fork lands bidi.
- Layout, hit-testing and raster sit below the UI layer with no egui dependency.
