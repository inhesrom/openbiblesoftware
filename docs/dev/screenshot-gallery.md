# Screenshot gallery (#222)

Every Tab kind, toolbar pane, menu, window, dialog and popover, each surface's empty,
loading and error states (#223), and Tabs in a narrow side Panel (#301), drawn headlessly in Light, Dark and Sepia at 1× and 2×, and
compared with its golden image (ADR 0019). The goldens are in
`docs/previews/gallery/`; [`docs/previews/gallery.md`](../previews/gallery.md) shows them all.

## Running it

    cargo xtask gallery           # draw and compare
    cargo xtask gallery --bless   # accept: write new and changed goldens, remove stale ones,
                                  # write docs/previews/gallery.md

`cargo xtask ci` runs the same tests in its `test` gate, so CI fails on a difference. The tests
are `crates/ui-egui/src/gallery_tests.rs` (`shell::tests::gallery::`); one test per section
(Tabs; toolbar panes, menus and windows; dialogs and popovers; menus and choosers in Tabs and
windows; empty, loading and error states; reading text) so they run in parallel.

## Reading a failure

A failure lists each image off its golden, with how many pixels differ, by how much at most,
and where:

    tab-settings-light-1x: 41230 of 1260000 pixels differ by more than 2 levels (up to 8), within x 0..1400, y 0..900
        golden: docs/previews/gallery/tab-settings-light-1x.png
        new:    target/gallery/new/tab-settings-light-1x.png
        diff:   target/gallery/diff/tab-settings-light-1x.png (golden | new | diff)

The diff image is the golden, the new image, and the golden faded to grey with every pixel
past the tolerance in magenta. In CI, `target/gallery/` is uploaded as the
`gallery-diff-<os>` artifact of the failing job. If the change is intended, bless and commit
`docs/previews/gallery/` and `docs/previews/gallery.md` with it.

## The tolerance

A pixel matches when no channel is more than 2 levels off; an image matches when no more than
0.01 % of its pixels differ beyond that (126 at 1×, 504 at 2×). It absorbs rounding across
platforms, not a change: the CI job `xtask gallery (seeded colour token change)` changes the red
of `LIGHT.shell` by 8 levels and checks that the gallery fails with diffs, then that `--bless`
updates the goldens and the gallery passes again.

## The fixtures

- A fresh launch for each surface and Look, with the accessibility audit's Starter subset
  (`release_set`, Nave's topical index, the Theographic events) and the BSB's Genesis 1.
- The clock fixed at 2026-11-09 10:30 UTC (`obs_store::clock::fix`), local time being UTC:
  Home, History, Reading plans and Back up & restore show the same day and times on every run.
  The launch's backup file is given that time too.
- The pointer parked in the window's bottom-right corner; the window 1400 × 900 points.
- The Import report imports from `../../target/gallery/usfm`, a path the same on every computer.

## Adding a surface

A new Tab kind does not build until the accessibility audit's `every_tab_kind` and the
gallery's `tab_title` name it; a new pane, menu or window in the audit's `every_overlay` is in
the gallery too. Other menus, popovers, dialogs and choosers are in `menus_and_toolbar`,
`dialogs_and_popovers` and `menus_and_choosers` in `gallery_tests.rs`: a name, what opens it,
and what shows it is open (the gallery fails if it is not on screen when drawn).

The empty, loading and error states are `pane_states` in `pane_states_tests.rs` (section
"Empty, loading and error states"): a name, what brings the state about (an empty Library, a
filter that matches nothing, jobs held so a Guide stays loading, a Guide section source or
fetch that fails), and what shows it is on screen. A surface that gains a new state adds it
there. The same file's tests check that no error state shows the error itself before Details,
that Retry asks again, that each empty state's Button fills its Tab, and that no Tab is blank in
an empty Library.

The reading text section (#305) is `reading_text` in `gallery_tests.rs`: the reading pane's text
where the visual QA sweep found it drawn wrong (a Commentary's italic words as the Starter's
converter makes them, a Highlight and the jump flash over several lines, a Note marker before
the next verse number). A rendering fix of the reading pane adds its case there.

Crowded charts (#304) are drawn with data the Starter subset has too little of: the Timeline
with `crowded_events` (`timeline_tests.rs`: more Events and Lifespans than the axis has rows
for, in the Tabs section, and a "+N" chip's menu in the menus section) and the Factbook Map of a
place with several Place locations (`factbook_maps` in `atlas_tests.rs`: Arimathea from the
geocoding excerpt). Each writes its index into the launch's library folder and adds it.

The Narrow Panels section is `narrow_panels` in `narrow_panels_tests.rs`: a Passage Guide, an
About page, a Text Comparison, the Atlas and the Timeline in a side Panel at its default width,
with the long titles, fields and summaries that once ran past its edge. The same file's tests
open each in the QA sweep's 1280 × 800 window and fail on any text or Resource-text glyph drawn
past the Panel's right edge, on a summary cut short, and on an Atlas label over a marker.

`every_window_popover_menu_and_chooser_is_in_the_gallery` keeps the gallery complete: it counts
each place `crates/ui-egui/src` opens a window, area, popup menu, context menu, menu or combo
box, and fails until `SITES` names the surface that draws a new one (or says why none does).
Then `cargo xtask gallery --bless`.

Left out: Report a problem (it shows the operating system and the log) and a font family's
style chooser (only for installed fonts, which differ by computer). The toolbar's right-click
menu is drawn once (from a Launcher; its background and grip open the same menu), as are a
Note card's menu (from its … button) and the Visual filters items (from the Filters menu).
