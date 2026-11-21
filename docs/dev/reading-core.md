# The reading core (`obs-reading`, L5)

`crates/reading` holds what a reading pane knows without egui (ADR 0021, plan
`docs/plans/mobile-ios.md` S01), so the desktop and the Mobile app read Resources the same way.
It depends on the engine and the text pipeline (`obs-layout`, `obs-fonts`, and `obs-render` for
tiles) and never on egui, eframe, winit or rfd:
`cargo tree -p obs-reading --edges normal | grep -c egui` is `0`, and `cargo xtask layers` and
`cargo xtask wasm` check it like every library crate.

## What lives there

| Module | What |
|---|---|
| `doc` | `ReadingDoc`, `DocBlock`: a Resource opened as one document (units, blocks, Bible Verses, Keys), and its `ComparedBible`/`CopiedBible` impls |
| `view` | `ReadingView`: virtualized blocks, heights, scrolling anchored to a block, jumps and glides, the Current location, Footnote markers, `Frame` |
| `heights` | `HeightIndex`: estimated then measured block heights |
| `selection` | `Selection`, `DocPos`, `Granularity`; selection rects, word at a point, copied text |
| `marks` | `Mark`, `MarkKind`, `marks_for`, `HighlightLooks`: Annotations over document positions |
| `interlinear` | `InterlinearSource`: what the lines under each Tagged word read |
| `workers` | `LayoutClient`, `SnippetCache`, `lay_out`: layout off the UI thread |
| `palette` | `TonePalette` over `Rgb`: the six Page tone constants, `derive`, `check`, `contrast`, `text_on`, `paint` (→ `StylePaint`), `color`, `overlays`, `inks`, `role_colours` (the tone as `obs_render::RoleColours`) (ADR 0012) |
| `tile` | `compose_tile`, `Tile`, `text_column`: a band of a `ReadingView`'s page drawn on the CPU into premultiplied RGBA8 (page colour, then glyphs through `obs_render::composite`), for the Mobile app (plan A4, option B; S03). Highlights, the selection and Note markers are not drawn yet |
| `reading_tokens` | The design tokens these modules measure with: `space`, `motion`, `size::TEXT_MIN`, `mark::FOOTNOTE_BOX` (ADR 0015) |

The feature `test-support` turns on the layout workers' hold and release hooks
(`ReadingView::hold_layouts`, `release_layouts`, `worker_layouts`) for tests in dependent
crates; `obs-ui-egui` enables it in its dev-dependencies only.

## What stays in `obs-ui-egui`

Everything that paints or takes egui input: the reading pane (`pane.rs`), the glyph atlas
texture, Text Comparison and Search views, Find (`find.rs`), Visual filters (`filters.rs`),
Cross-references (`references.rs`), the Shell and every Tool. `obs_ui_egui::Palette` is
`TonePalette` in egui's `Color32` (built by `From<TonePalette>`, its methods converting and
asking `TonePalette`), and `obs_ui_egui::tokens` re-exports `reading_tokens` beside the chrome's
tokens. `obs_ui_egui` re-exports the reading core's public names, and its `lib.rs` brings the
moved modules in by name (`use obs_reading::{doc, view, …}`), so `crate::view::ReadingView`
and the like still resolve in the painting code.

## Rules

- No egui type in `crates/reading`; a colour is `obs_engine::Rgb`, a rect `[f32; 4]`.
- The design-token lint (`crates/ui-egui/src/design_tokens_tests.rs`) scans `crates/reading/src`;
  its allowlist names those files `reading/<file>`. A new measurement goes in
  `reading_tokens.rs` (see `docs/dev/design-tokens.md`).
- A change here is a change to what the desktop draws: the Screenshot gallery
  (`cargo xtask gallery`) and `cargo xtask bench` cover it, and its PR is a UI change (ADR
  0020 rule 4).
