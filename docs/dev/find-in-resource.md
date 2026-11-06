# Find in Resource (#175)

Ctrl+F in a Resource Tab (Bible, Commentary, Lexicon, Dictionary) or a Text Comparison Tab
opens the **Find bar** under the Tab's header strip (also: the Tab's ⋯ menu → Find…). It finds
words in that one Resource; the Search Tool finds them across the library.

## Behaviour

- Typing finds every match in the **whole** Resource (every row of a Text Comparison) on a
  worker thread (`obs-find`; inline on wasm). A new keystroke cancels the search in flight.
- The count reads "12 of 48" ("Finding…" while the worker runs, "No matches" when none).
  When matches arrive, the current one is the first at or after the top of the view.
- Enter / ▼ go to the next match, Shift+Enter / ▲ to the previous, wrapping around. A match
  out of view is scrolled to the top of the pane; one already in view is left where it is.
  A Text Comparison brings the match's Verse block to the top, then, once the block is laid
  out, scrolls within it until the match's line is in view (a block with many Bibles can be
  taller than the pane). The keyboard stays in the field.
- A Find step that scrolls counts as the user scrolling: the Tab's Link set follows. It
  makes no History Visit.
- Esc (in the field, or in the focused Tab when nothing else has the keyboard) or × closes the
  bar and clears every mark. Ctrl+F again opens a new, empty bar.
- Ctrl+F with a short one-line selection in the pane finds the selected words.
- Alt+Enter or **Search all Resources** runs the text, quoted as a phrase, in the Search
  Tool: Bible mode over every Bible from a Bible or Text Comparison Tab, Library mode (every
  Commentary, Lexicon and Dictionary) from the others. It reuses the most recent Search Tab
  but sends the whole scope (Whole Bible; every Commentary, Lexicon and Dictionary), so a
  scope narrowed there earlier does not carry over.
- Commands: `tab.find` (Ctrl+F, optional `text`), `tab.find_next`, `tab.find_previous`
  (`OBS_SCRIPT` can drive them: `10 tab.find {"tab":"focused","text":"love"}`).

## Matching (`obs_search::find`)

- The Search normalizer (`obs_model::fold_for_find`): NFD, final forms (ς σ, ך כ, ם מ, ן נ,
  ף פ, ץ צ) and curly quotes folded; case folded unless **Match case**; combining marks
  (Greek accents and breathings, Hebrew points and cantillation, Latin diacritics) dropped
  unless **Ignore diacritics** is off. With it off, typed and stored marks are compared in
  canonical order.
- Matches are substrings ("love" finds loved, loveth); **Whole word** requires no letter,
  digit or mark on either side.
- A match never splits a letter from its marks: both ends fall on cluster boundaries.
- With Ignore diacritics on, a query in Latin letters also finds Greek and Hebrew words by
  their transliteration in the shared scheme (`transliteration_key`): "agape" finds ἀγάπη,
  "bara" finds בָּרָא. These matches are always whole words (as a prefix "the" would find
  every θεός).

## Drawing

- Marks are an overlay, never a Highlight style or an Annotation: every match in the
  `find_match` palette role, the current one in `search_hit` with a `find_current` outline
  (WCAG: every ink on `find_match` ≥ 4.5:1, the outline ≥ 3:1 on the page and on
  `search_hit`; see `every_ink_on_every_highlight_meets_wcag_aa_in_every_tone`).
- Rectangles come from the laid-out clusters each frame (`pane::found_rects`), like the
  selection and Highlights, so they follow scrolling, Text size, width and Page tone. Only
  the matches reaching into the blocks laid out in view are looked at (binary search), so a
  Resource with tens of thousands of matches costs the frame nothing extra.
- A match covers letters only: an inline verse number anchored at its first byte is not
  marked (`text_range_rects`).
- They draw over Highlights and under the selection.

## Decisions

- Find stays UI state (`TabUi.find`): it is not saved in Layouts or the Last session.
- The Find bar is a second row of the Tab header (it wraps in narrow Panels) rather than a
  floating box, so it never covers the text.
- Typing Hebrew in the field shows the chrome font's missing-glyph boxes: egui's interface
  fonts have no Hebrew and no bidi. This is the same for every chrome text field (the
  Lexicon box, the Search box) and is not changed here; the marks in the text are right.

## Evidence

- `obs-search` `find_tests`, `obs-model` `find_folding_*`: options, Greek, Hebrew, final
  forms, transliteration, cluster boundaries.
- `obs-engine` `ctrl_f_opens_the_find_bar_of_a_resource_or_text_comparison_tab`.
- `obs-ui-egui` `shell::tests::find_bar::*` (real egui events through the shell harness):
  KJV John "love" count, Enter through every hit in order, Esc; Whole word / Match case;
  Greek and Hebrew without marks; highlights after a Text size change; Text Comparison rows;
  Alt+Enter hand-off; Ctrl+F with a selection; the ⋯ menu; a hit at a verse start.
- `real_kjv_john_love_counts_and_enter_visits_every_hit_in_order` (ignored in CI; needs
  `OBS_KJV_OBR`): on the Starter library's KJV, 646 hits of "love" (57 in John; John's first
  is 363 of 646), Enter visits all 646 in order.
- `cargo xtask bench` draws a Find bar's matches ("the", tens of thousands) over every pane
  each frame.
- Previews: `docs/previews/find-*-kittest.png` (headless shell harness) and, from the real
  app on macOS through `OBS_SCRIPT`/`OBS_SHOT`, `find-kjv-john-app.png` (KJV, "love", 366 of
  646 at John 5:20) and `find-tagnt-agape-app.png` (TAGNT, "αγαπη" typed without accents, 171
  of 199 at 1 John 4:7).
