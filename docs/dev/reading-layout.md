# Reading layout (#209)

**View → Reading layout** sets how the focused Resource Tab's text flows. Each choice also has
a default in **Settings → Reading layout defaults**; a Tab follows a default until it chooses
for itself, and **Use defaults** forgets its own choices.

| Option | Choices | Default | Tab command parameter | Default's command |
|---|---|---|---|---|
| Paragraphs or One verse per line | `paragraphs`, `verse_per_line` | Paragraphs | `flow` | `view.text_flow {value}` |
| Large chapter numbers | on, off | off | `large_chapter_numbers` | `view.large_chapter_numbers {on}` |
| Section headings | shown, hidden | shown | `section_headings` | `view.section_headings {on}` |
| Reading column width | `narrow` (30 ems), `medium` (38), `wide` (50), `full` | Medium | `column` | `view.column_width {value}` |
| Justified text | on, off | off | `justified` | `view.justified {on}` |

`tab.reading_layout {tab, …, reset}` sets any of a Tab's own choices at once (the menu runs it
for the focused Tab). A Tab's choices are `obs_engine::TabReadingLayout` (each `None` follows
the default) and are saved in `TabSnapshot::reading_layout`, so Layouts and the Last session
keep them; the defaults are `TextSettings::layout`, kept with the other View preferences in the
User database (`reading_layout` in the `view` preference record).

## In the layout pipeline

The document's blocks never change: every option is applied when a block becomes a
`Paragraph` (`obs_layout::block_paragraph`) and when it is laid out (`obs_layout::layout`), so
the height index, the scroll anchor and the Current location work as before. The four options
that change line breaking are part of `TextSettings::key`; changing one re-estimates heights and
lays out again at the Current location (`ReadingView::set_settings`), exactly as a Text size
change does. The column width is not in the key: it only changes the width the pane gives the
view (`ReadingView::set_width`), which keys the height index on its own.

- **One verse per line**: in a Bible's paragraphs, each verse start is a `Paragraph::line_starts`
  run, so a line ends there whether or not verse numbers are shown. Lines start at the margin
  (no first-line indent); paragraph spacing is kept.
- **Large chapter numbers**: when a prose paragraph opens with a chapter's first verse, the
  chapter number becomes `Paragraph::drop_numeral`, sized so its figures reach from the first
  line's figures to the second line's baseline. `drop_numeral_scale` is a first guess from
  ordinary lines; `layout` then fits the size to the lines as laid out (measuring the
  figures' ink with `obs_fonts::ink_top`), so Interlinear lines under the first line, or
  Hebrew metrics, make it taller rather than pushing it below the opening text. The first
  two lines are inset by its width on the paragraph's start side (the right in Hebrew); a
  one-line paragraph grows to hold it. It hit-tests as the chapter's first verse number over
  the whole box it is drawn in, beside the second line too and below a one-line paragraph,
  and a selection or Highlight over the first verse marks that whole box
  (`LaidBlock::range_band`). Poetry keeps the inline chapter number: its lines are blocks of
  their own.
- **Section headings hidden**: a Bible's `heading` blocks become `Paragraph::hidden`, laid out
  with no lines and no height. Other Resources' headings are their structure and stay.
- **Justified text**: prose, list and plain paragraphs (not poetry, headings or titles) have
  `Paragraph::justify`; every line but a paragraph's last and a verse's last (with One verse
  per line) spreads its room over its ordinary spaces (not the no-break spaces after verse
  numbers). In right-to-left text the room goes before each word, where its space is drawn.
  The space's cluster covers its room, so hit-testing, the caret and highlights treat it as
  the space.
- **Hyphenation**: the reading view sets `Paragraph::hyphenate` for justified text of an
  English Resource (`hyphenates`: language `en`, `en-*` or `eng`). A word that does not fit is
  split at the last English hyphenation point (the hyph-en-us patterns built into the `hypher`
  crate, at least two letters before and three after) whose head and a hyphen fit. A word wider
  than a whole line is split the same way at the start of its line, and its tail again while it
  is still too wide, always at the whole word's hyphenation points; a word with no point that
  fits is broken between grapheme clusters like any overwide text. Only a plain
  left-to-right run of ASCII letters is hyphenated, so Greek, Hebrew and right-to-left text
  never are. The hyphen is drawn but is not text: it has no clusters, so selection, copying,
  Find and Annotations see the word whole. A word with soft hyphens of its own breaks only at
  them.

The `reading-layout` bench scenario (`docs/dev/bench.md`) scrolls 4 panes with every option on.
