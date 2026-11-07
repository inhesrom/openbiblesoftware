# Interface icons are one SVG line set, rasterized per size and display scale

The interface used emoji (✏, 🔎, 📌, 👁, ⚠) and assorted symbols (★ ☆ ✓ × ‹ › ▶ ◀ ▾ ⏷ ≡ »), drawn from egui's fallback emoji font or painted ad hoc, plus four painted toolbar drawings. They rendered unevenly, clashed with the theme and had no common weight (#220). We decided:

1. **One icon set, from Lucide.** Every icon is an SVG from Lucide 1.70.0 (ISC; the icons Lucide took from Feather are MIT), copied unmodified into `crates/ui-egui/assets/icons/` with its `LICENSE` (two are filled copies of `star` and `square`). All are drawn on Lucide's 24-unit grid with its one 2-unit stroke, round caps and joins. They are credited in `ATTRIBUTION.toml` and shown with their licence text in Help → Credits. Lucide is a general-purpose open set; no icon, name or look comes from a commercial Bible program.
2. **resvg rasterizes them in the app, per size and display scale.** `obs_ui_egui::icons` keeps one texture per icon and size in physical pixels (16 points at 1.5× is 24 pixels), made on first use with resvg (no text, fonts or raster-image features) as a white coverage mask, drawn texel for pixel without filtering and tinted with a colour role of the chrome theme (or of the page palette on Resource pages). A new display scale or Interface size makes new textures.
3. **No emoji or symbol glyph stands in for an icon.** A test scans the string literals of `obs-ui-egui`, the desktop app and `obs-engine` (whose labels and messages the interface shows) for emoji code points (Unicode `Emoji` and `Extended_Pictographic`), pictograph and shape blocks, and a symbol standing alone at the start or end of a label.

## Considered Options

- **Our own drawings, painted with egui shapes:** crisp at any scale with no new dependency, as the four Launcher drawings were; but some sixty icons drawn as code are hard to keep in one weight and hard to review, and the ticket asked for SVG.
- **egui_extras's SVG image loader:** also resvg underneath, but it caches by URI and size hint, not display scale, adds the image-loader machinery and resamples when the scale changes.
- **An icon font:** one texture atlas through egui's text path, but tinting and sizing on the type scale come free only for glyphs; a font cannot be credited per icon, is harder to extend, and still needs a built font file.
- **Keeping egui's emoji fallback font for icons:** the problem the ticket names.

## Consequences

- `resvg` and its small pure-Rust dependencies (`usvg`, `tiny-skia`, `svgtypes`, …) join the UI layer; they build for wasm32 too and are listed in Third-party software.
- Each icon at each size costs one small texture (24 × 24 pixels for a 16-point icon at 1.5×); the set is about sixty icons and only those drawn are made.
- Lucide's strokes sit on whole grid units, so they land exactly on pixels only at multiples of 24 pixels; at 16 points on a 1× display a stroke is antialiased over two pixels, as in a browser. Icons are never resampled from another size.
- Tests find an icon where it is drawn by its marker (`[pin]`, `[x]`) in the harness's list of painted text.
- egui's emoji font stays installed: user text (a Note, a Collection's name) may contain emoji.
