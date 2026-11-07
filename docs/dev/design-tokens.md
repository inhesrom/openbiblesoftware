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

Colour roles: `tokens::Chrome` with `LIGHT` and `DARK` (read the current one with
`theme::c()`), `MAP_LIGHT`/`MAP_DARK` for the Atlas, `HIGHLIGHT_PRESETS` for the Highlight
style swatches. Page colours (ink, paper, Highlight tones) are `Palette`.

## Components

| Component | Function |
|---|---|
| Button | `components::button(atoms)` |
| IconButton | `components::icon_button(ui, icon, name)` |
| TextField | `components::text_field(&mut text)` |
| SearchField | `components::search_field(&mut text, hint, width)` |
| Toggle | `components::toggle(ui, &mut on, label)` |
| Menu | `components::menu(ui, title, contents)` |
| Popover | `components::popover_frame()` |
| Card | `components::card_frame()`; a notice: `notice_frame(colour)` |
| SectionHeader | `components::section_header(ui, title)` |
| Tab page, band, chip | `page_frame()`, `band_frame()`, `chip_frame()`, `removable_chip(ui, label, what)` |
| Text styles | `title_text`, `subtitle_text`, `label_text`, `hint_text`, `caption_text` |
| Painted helpers | `hover_outline`, `scroll_thumb`, `text_placeholder`, `rule_stroke` |

## The lint

`tokens::tests::no_spacing_size_or_colour_literals_outside_the_tokens` fails on a literal
spacing, size or colour anywhere in the crate outside `tokens.rs` and `palette.rs`. Use a
token, add a named one to `tokens.rs`, or, for a number that is not part of the look (a
duration, a zoom speed, a height estimate, a hit test's tolerance), add an entry with its
reason to the test's `ALLOWED` list.

A bare ½, 1 or 2 passes only as a factor, a bound or a tolerance (`/ 2.0`, `* 0.5`,
`.clamp(0.0, 1.0)`, `.abs() < 0.5`). Added to or taken from a coordinate it is a spacing:
`icon.right() + space::XXS`, `last[2] + space::PX`, and `rect.bottom() - stroke::HAIR / 2.0`
to centre a hairline on a pixel.

## Previews

The before (left) and after (right) images of #221 are in `docs/previews/tokens-*.png`. The
Bible pane, the Tools launcher pane, the Passage Guide and Settings, light and dark, are the
real app on the Mac (`OBS_SCRIPT` + `OBS_SHOT`); every other surface is
`tokens-<surface>-kittest.png`, rendered headlessly by

    UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib design_token_previews

into `target/previews/tokens/`, once on master and once on the branch.
