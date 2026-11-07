# The interface's look is design tokens and shared components, checked by a literal lint

About forty tickets added the interface a surface at a time, each with its own spacing, type sizes, corner radii, frames and headings. Tab pages were inset 14 × 10, 18 × 12 or 18 × 14 points, chips were drawn in five places, and section headings in six (#221). We decided:

1. **One token module.** `obs_ui_egui::tokens` holds the spacing scale (`space`: 2, 4, 6, 8, 12, 16, 24 and 32 points), the type scale (`text`: 11, 12, 13, 14, 16, 20 and 24 points), corner radii (`radius`), stroke widths (`stroke`), elevations (`elevation`, the window and popover shadows), frame insets (`inset`), control and column sizes (`size`), washes (`alpha`), the reading page's marks (`mark`), chart and map geometry (`chart`), shares of the room available (`share`), multiples of the Resource text size (`scale`), and the chrome's colour roles (`Chrome`, `LIGHT`, `DARK`, the Atlas's map colours and the Highlight preset swatches). Page colours stay in `Palette`, the other colour token set. `theme` installs the tokens as egui's style.
2. **Shared components.** `obs_ui_egui::components` holds the pieces every surface is built from: Button, IconButton, TextField, SearchField, Toggle, Menu, Popover, Card, notices, the Tab page, bands, chips (with a Close button), SectionHeader, the text styles (title, subtitle, label, hint, caption), the hover outline, the scroll bar thumb and the text placeholder. A surface uses one of these rather than restyling an egui widget itself.
3. **A lint test keeps it so.** `design_tokens_tests.rs` reads every non-test source file of the crate (comments, strings and `#[cfg(test)]` items blanked) and fails on a numeric literal in a call that takes a spacing, size, radius, stroke, font size or colour, on any other float literal except the unitless 0, ½, 1 and 2, and on a named `Color32` other than `TRANSPARENT` and `PLACEHOLDER`. The token files themselves and the benchmark harness are exempt. What is not part of the look (a duration, a zoom speed, a map's default bounds, a height estimate, an OS window's placement) is listed in the test's allowlist with its reason, and an entry that stops matching fails the test.

## Considered Options

- **egui's `Style` alone:** egui's spacing and visuals cover its own widgets, but most of our surfaces are painted (the toolbar, the reading page, Guide sections, the Atlas, the Timeline) and need sizes egui's style has no field for.
- **A token module without a lint:** cheapest, but nothing stops the next ticket writing `add_space(10.0)` again; the inconsistency the ticket names came from exactly that.
- **Clippy's `disallowed_methods` or a custom lint:** clippy cannot tell a spacing literal from any other argument, and a rustc lint plugin is a nightly-only tool for one crate.

## Consequences

- Values were normalized to the scales, so most surfaces moved by a point or two (10-point gaps became 12, chips and notices have one inset, cards one frame); behaviour did not change. One test's window width was adjusted because its synthetic text now wraps at a different width.
- A new surface needs either an existing token or a new, named one in `tokens.rs`; the lint's failure message says so.
- The lint is a text scan, not a type check: an integer literal outside a sink call (`const RADIUS: u8 = 7`, then `corner_radius(RADIUS)`) slips through. Review still applies.
