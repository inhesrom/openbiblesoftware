# Design tokens and shared components (#221)

Every surface takes its measurements and colours from `crates/ui-egui/src/tokens.rs` and is
built from the pieces in `crates/ui-egui/src/components.rs` (ADR 0015). This page is the short
guide to using them.

## Tokens

| Module | What | Values |
|---|---|---|
| `space` | Gaps, insets, indents | `PX` 1, `XXS` 2, `XS` 4, `S` 6, `M` 8, `L` 12, `XL` 16, `XXL` 24, `XXXL` 32 |
| `text` | Type sizes of the chrome | `CAPTION` 11, `SMALL` 12, `LABEL` 13, `BODY` 14, `SUBTITLE` 16, `TITLE` 20, `DISPLAY` 24, `MONO` 12 |
| `radius` | Corner radii | `XS` 2, `S` 4, `M` 6 (controls, cards), `L` 8 (menus, popovers), `XL` 10 (windows), `PILL` (half a chip) |
| `stroke` | Line widths | `HAIR` 1, `MEDIUM` 1.5, `THICK` 2 |
| `elevation` | Shadows | `window(dark)`, `popup(dark)` |
| `inset` | Frame margins | `PAGE`, `CARD`, `NOTICE`, `POPOVER`, `WINDOW`, `BAR`, `BAND`, `CHIP`, `MENU_BAR`, `WORKSPACE` |
| `size` | Controls, rows and columns | icons, `CONTROL` 28, `ROW` 28, `ROW_COMPACT` 24, chips, fields, columns, dialogs, breakpoints |
| `alpha` | Washes over a fill | `WASH`, `TINT`, `HALF`, `SOFT`, `STRONG` |
| `mark` | Marks on the reading page, in ems | Highlight bands, underlines, boxes, Note markers, margin marks |
| `chart` | Timeline, Atlas and Search charts | rows, bars, markers, tints |
| `share` | Fractions of the room | side panes, dialogs, lists |
| `scale` | Multiples of the Resource text size | line height, headwords, Note headings, placeholders |
| `motion` | Durations (seconds) and easings (#225) | `POPOVER` 0.12 (and egui's own transitions), `PANEL` 0.18, `JUMP` 0.28, `SCROLL` 0.14, `FLASH` 1.2, `SHIMMER` 1.4; `ease_out`, `ease_in_out`, `progress` |

Colour roles: `tokens::Chrome` with `LIGHT` and `DARK` (read the current one with
`theme::c()`), `MAP_LIGHT`/`MAP_DARK` for the Atlas, `HIGHLIGHT_PRESETS` for the Highlight
style swatches. Page colours (ink, paper, Highlight tones) are `Palette`.

Motion is driven by frame time (egui's input time) and never runs with Reduce motion on
(`crate::motion`: `reduced(ctx)`, the pop-up, scroll and shimmer helpers); a new animation
takes its duration and easing from `tokens::motion` and checks `motion::reduced` (or is only
started when motion is not reduced).

## Components

| Component | Function |
|---|---|
| Button | `components::button(atoms)` |
| IconButton | `components::icon_button(ui, icon, name)` |
| TextField | `components::text_field(&mut text)` |
| SearchField | `components::search_field(&mut text, hint, width)` |
| Toggle | `components::toggle(ui, &mut on, label)`; greyed out: `toggle_enabled(ui, enabled, &mut on, label)` |
| Radio | `components::radio(ui, selected, label)`, `radio_value(ui, &mut current, value, label)` |
| Menu | `components::menu(ui, title, contents)` |
| Popover | `components::popover_frame()` |
| Card | `components::card_frame()`; a notice: `notice_frame(colour)` |
| SectionHeader | `components::section_header(ui, title)` |
| Tab page, band, chip | `page_frame()`, `band_frame()`, `chip_frame()`, `removable_chip(ui, label, what)`; a Guide's page: `guide_page_frame(palette.page)` |
| Text styles | `title_text`, `subtitle_text`, `label_text`, `hint_text`, `caption_text` |
| Painted helpers | `hover_outline`, `scroll_thumb`, `text_placeholder`, `rule_stroke` |

Check boxes and radio buttons are always the Toggle and Radio components (#306): they outline
the box and the circle in Muted (egui's Rule border nearly vanishes on a Field-coloured dialog
in Dark) and draw the check mark as thick as the focus ring.
`tokens::tests::every_check_box_and_radio_button_is_a_shared_component` fails on an egui
`checkbox`, `radio` or `radio_value` anywhere else.

A Guide's page is the Page tone, as its Resource snippets are, so they sit on the page and not
on slabs of their own (#306); when the chrome's text does not read on the tone (Paper chosen in
a dark Theme) it stays the field and each snippet keeps its own page
(`theme::text_reads_on`).

## The lint

`tokens::tests::no_spacing_size_or_colour_literals_outside_the_tokens` fails on a literal
spacing, size or colour anywhere in the crate outside `tokens.rs` and `palette.rs`. Use a
token, or add a named one to `tokens.rs`.

No value is exempt as such. The rule lets through only what cannot be a measurement: 0,
halving or doubling a length (`w / 2.0`, `h * 0.5`, `2.0 * MARGIN`), the rest of a fraction
(`1.0 - t`) and, inside a call, an integer 1 that counts (`level.saturating_sub(1)`). A ½, 1
or 2 added to or taken from a coordinate is a spacing: `icon.right() + space::XXS`,
`last[2] + space::PX`, and `rect.bottom() - stroke::HAIR / 2.0` to centre a hairline on a
pixel. Inside a spacing or size call a 1 is a size (`rect.expand(space::PX)`, not
`expand(1.0)`).

A number that is not part of the look (a duration, a zoom factor, a clamp bound, a hit
test's tolerance, a default before the first layout) gets an exact entry in the test's
`ALLOWED` list: the file, the whole line (trimmed), the literal and the reason. The entry
covers that line only; editing the line flags it again, and an entry that no longer matches
fails the test as stale.

## Previews

The before (left) and after (right) images of #221 are in `docs/previews/tokens-*.png`. The
Bible pane, the Tools launcher pane, the Passage Guide and Settings, light and dark, are the
real app on the Mac (`OBS_SCRIPT` + `OBS_SHOT`); every other surface is
`tokens-<surface>-kittest.png`, rendered headlessly by

    UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib design_token_previews

into `target/previews/tokens/`, once on master and once on the branch.

Since #222 the Screenshot gallery (`docs/dev/screenshot-gallery.md`) draws every surface in
Light, Dark and Sepia at 1× and 2× and fails CI when one changes; a change to a token re-blesses
its goldens with `cargo xtask gallery --bless`.
