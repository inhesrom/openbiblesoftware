# Footnote markers and popovers (#208)

A Resource's own notes (USFM `\f` and `\x`, OSIS and ThML `<note>`, GBF `RF`, STEPBible
variant notes) are imported as **Footnotes**: one `note` text unit each (`note.<n>`), with a
zero-width `note_ref` span where it belongs in the text (ADR 0003). A Resource Tab shows each
one as a **Footnote marker**:

| `note_ref` | Marker |
|---|---|
| footnote | a raised italic letter in the Page tone's marker colour: a, b, c… counted from each chapter's first Footnote (aa after z; a unit without Verses counts from its start) |
| cross-reference, with its zero-width `xref` beside it | the `↗` of that Cross-reference only (its hover already previews the Passages) |
| cross-reference without one | a raised `‡` |

Hovering a marker for Word Info's dwell (`HOVER_DWELL_SECS`), or giving it keyboard focus with
Tab, shows its popover under it: "Footnote a · John 1:5" and the note, drawn with the text
pipeline like the text it annotates (italic `\fqa` stays italic). The popover stays while the
pointer is on the marker or on the popover. Clicking the marker (Enter from the keyboard) pins
it; a pinned popover closes on Esc (focus goes back to the marker), its ×, a click outside it,
or another click on the marker; resting the pointer on another marker, in any pane, leaves it
(clicking that marker replaces it). Pinned from the keyboard, focus moves to its first link (its
× when it has none) as soon as the note's text is laid out, even when Enter came before the dwell.

Bible references in a Footnote are links: the `scripture_ref` and `xref` spans the importer
made, plus written references found in its text when it is shown (`obs_engine::body_references`,
the Notes Tool's finder; "Cited in Hebrews 4:4", BSB's `\ref Isaiah 40:3|ISA 40:3\ref*`, whose
text the USFM importer keeps). Found and `scripture_ref` targets are read in the Resource's own
numbering and go through its Verse map, like a reference in its text. Hovering (or focusing) a
link shows the reference popup with the ★ Bible's text; clicking it goes there
(`reference_navigate`, as a reference in the text does) and closes the popover.

**View → Footnote markers** (`view.footnote_markers`, params `tab` and `on`) shows or hides the
markers of the focused Resource Tab only. `Tab::footnote_markers` is on by default; a Tab that
hid them saves `footnote_markers_hidden` in Layouts and the Last session. Hidden, the layout
lays out no `note_ref` marker and no zero-width `xref`/`scripture_ref` `↗` (`TextSettings::
footnote_markers`, part of the settings key, so views lay out again).

## Contract

- Layout (`obs_layout::present`): markers are `InlineObject::NoteMarker` runs anchored at their
  byte, not unit text. Selection, Copy (with reference, #34), Copy verses (#154), Find and
  Annotation anchors work in unit bytes, so they never include a marker or its Footnote. In a
  right-to-left paragraph a letter is followed by a right-to-left mark so it stays beside the
  text it follows, not the verse number after it.
- `ReadingDoc` keeps the note units (`ReadingDoc::footnote`) out of its blocks.
- `ReadingView::footnote_markers(height)` lists the markers laid out in view with their boxes;
  the reading pane makes each one a small click widget with an AccessKit name ("Footnote a",
  "Cross-reference note") and the note as its description, and reports `hovered_footnote` (the
  pointer's, else the focused one) and `clicked_footnote`.
- `shell/footnotes.rs` owns the dwell, pinning and drawing; the popover's text, × and links
  have AccessKit names.

## Evidence

- `crates/ui-egui/src/footnotes_tests.rs` on BSB Isaiah 40:1-3 and John 1:1-23 from the official
  USFM (`crates/formats/tests/assets/usfm/bsb/`).
- `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui footnote_previews` writes the harness images under
  `target/previews/`; `docs/previews/footnotes-*.png` are those, converted.
- `cargo xtask bench`: the synthetic Bible ends every sixth verse with a Footnote marker.
