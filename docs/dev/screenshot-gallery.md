# Screenshot gallery (#222)

Every Tab kind, toolbar pane, menu, window, dialog and popover, drawn headlessly in Light, Dark
and Sepia at 1× and 2×, and compared with its golden image (ADR 0018). The goldens are in
`docs/previews/gallery/`; [`docs/previews/gallery.md`](../previews/gallery.md) shows them all.

## Running it

    cargo xtask gallery           # draw and compare
    cargo xtask gallery --bless   # accept: write new and changed goldens, remove stale ones,
                                  # write docs/previews/gallery.md

`cargo xtask ci` runs the same tests in its `test` gate, so CI fails on a difference. The tests
are `crates/ui-egui/src/gallery_tests.rs` (`shell::tests::gallery::`); one test per section
(Tabs; toolbar panes, menus and windows; dialogs and popovers) so they run in parallel.

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
the gallery too. A dialog or popover the audit does not open goes in `dialogs_and_popovers` in
`gallery_tests.rs`. Then `cargo xtask gallery --bless`.

Left out: Report a problem (it shows the operating system and the log).
