# Note display and the Note margin (#210)

A Resource Tab shows the Notes in its text one of three ways, its **Note display**
(`obs_engine::NoteDisplay`, View → Note display, `view.note_display` with `value` `margin`,
`icons` or `hidden` and an optional `tab`, the focused one by default):

| Note display | What the reading pane draws |
|---|---|
| Icons (the default) | a small Note marker at the end of each Note's words, or beside the verse number for a Note made in another Bible (#63) |
| Margin | the markers, and a card for each Note in the **Note margin** beside the text |
| Hidden | no Notes (Highlights, and the Notebooks' Show in text, are untouched) |

It is the Tab's own choice, like its Footnote markers (#208): a Layout and the Last session keep
it (`TabSnapshot::note_display`, left out while it is Icons), and it has no default in Settings
(`every_preference_command_is_a_setting` exempts it). View → Note display is greyed out unless a
Resource Tab is focused.

## Where the margin and its cards go

`crates/ui-egui/src/note_margin.rs` (pure, unit-tested; its measurements are design tokens, `tokens::note_margin`, #221):

- `split(room, size, column, rtl)`: the Note margin is 40% of the Reading column's own width
  (#209; at Full width, of the pane's room), between `CARD_MIN` (170 pt) and `CARD_MAX`
  (340 pt). The text column takes what is left up to its own width, and text, `GUTTER` and
  margin are centred together. The margin is right of left-to-right text and left of
  right-to-left text. Narrower than that, the margin shrinks to keep `MIN_TEXT_EMS` (16 ems)
  of text, down to `CARD_MIN`; below that `split` is `None` and the pane shows Icons (the Tab
  keeps Margin, and the cards come back when the pane is wide enough).
- `stack(wanted, heights)`: each card wants to start on the line of its Note's marker; one
  that would overlap the card above starts `CARD_GAP` below it, so cards keep text order and
  never overlap.

The reading pane (`pane.rs`) lays its text column out at the split's width, reports the margin
(`PaneOutput::note_margin`) and every Note marker it drew (`PaneOutput::note_markers`); in
Hidden it draws no Note marks. `crates/ui-egui/src/shell/note_margin.rs` then draws the cards
in that frame, from those markers, so cards move with the text as it scrolls or lays out again,
with no state of their own but each card's measured height and which cards are expanded
(`MarginUi`, per Tab). A card whose marker is not laid out in view (above the pane's first
block, or below its bottom) is not drawn.

## Cards

- A **leader line** runs from the text column's edge, on the marker's line, across the gutter
  to the card; a stacked card's slopes down to it. Hovering a card outlines its marker and
  darkens its leader.
- **Collapsed**, a card shows the Note's first three rows (headings strong, list items with
  their bullets or numbers, quotes in quotation marks), cut with "…". Clicking it, or its
  chevron, **expands** it to the whole Note; the chevron collapses it again.
- Clicking an expanded card's text **edits it in place** with the Note editor (#186). The edit
  is the Notes Tool's `CardEdit` at `EditPlace::Margin(tab)`: it saves as you type
  (`AUTOSAVE_SECS`), and ends (saving) when the editor loses the keys (a click elsewhere, Esc),
  on Ctrl+Enter, or in any frame its Tab is not drawn. One Note is edited at a time: starting
  to edit it here ends an edit in the Notes Tool, and the other way round.
- The wheel over a card scrolls the text, as anywhere else on the pane.
- Cards are on the page, so they take the Page tone (fill and border mixed from the page and
  the Note marker colour, text in the page's ink).

Tests: `note_margin::tests` (widths, narrow panes, stacking) and
`crates/ui-egui/src/note_margin_tests.rs` (three Notes in Romans 8 aligned while scrolling in
the KJV and the WEB; stacking, expanding and editing in place; Icons in a narrow Tab; Hidden
and the View menu); `view_note_display_sets_one_resource_tab_and_is_kept_in_layouts` in the
engine.
