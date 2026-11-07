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
or another click on the marker. Esc closes a hover popover too. A marker whose popover was
dismissed (Esc, another click on it, a click elsewhere, its ×, a link followed) shows none while
the pointer or focus stays on it, however long it rests there, even after another marker's
popover has shown meanwhile (the pointer on another marker, the focus still on this one): it
shows again once both have left it, or at once on a click (Enter). Resting the pointer on another marker, in any pane, leaves
a pinned popover (clicking that marker replaces it). A shown popover has first refusal on Esc: a
command bound to Esc (Unpin Word Info) waits for the next Esc while one is on screen in that
window, and the popover consumes the Esc it closes on, so no floating window behind it closes too.
Space on a focused marker activates it; no reading pane pages on that Space, neither the
marker's nor another one under the pointer. A note
taller than `MAX_BODY_HEIGHT` (or than the window allows) scrolls inside the popover, which stays
in the window; a link given keyboard focus is scrolled into view.

Each Tab's popover is one state machine, `FootnoteState` in `crates/ui-egui/src/shell/footnotes.rs`:
its `FootnotePhase` (`Closed`, `Dwelling` waiting for the dwell, `Open` shown on hover, `Pinned`)
and, apart from it, the set of `dismissed` markers, each kept until the pointer is off it and it
does not have the keyboard focus (hovering another marker never clears it). The reading pane's
frame turns what the pointer, keyboard and text did into `FootnoteEvent`s, and every change goes
through `next(state, event)`, whose unit test covers each phase with each event, with a marker
dismissed and while another one's popover shows; a second table walks two panes' Tabs through a
focused marker in one and the pointer in the other. Every Footnote widget drawn (a marker, a
popover's link or ×), in any pane, is registered for its pass (`register_footnote_widget`); a
pane pages on Space only when egui's focused widget is none of those drawn this pass or the last
(`footnote_widget_focused`), so Space goes to a focused Footnote widget, and no record of who had
the focus outlives the frame. The registry is kept per viewport: a native floating window shares
the main window's egui context but counts its own passes and has its own focus. Esc moves the focus back to the marker in the same frame. `Footnotes::send` keeps
one popover pinned at a time across Tabs. Pinned from the keyboard (Enter or Space on the focused
marker, or a screen reader's click: the marker widget reports it, wherever the pointer rests),
focus moves to its first link (its × when it has none) as soon as the note's text is laid out,
even when Enter came before the dwell. A popover belongs to its Tab and Resource: it closes when
its Tab closes, shows another Resource, or the Workspace is rebuilt (a Layout loaded), and a
marker is the same only in the same Resource (two Resources share unit, anchor and note ids).

Bible references in a Footnote are links: the `scripture_ref` and `xref` spans the importer
made, plus written references found in its text when it is shown (the Notes Tool's finder read
in the Bible's own numbering, `obs_engine::body_references_in(text, &doc.versification)`; "Cited
in Hebrews 4:4", BSB's `\ref Isaiah 40:3|ISA 40:3\ref*`, whose text the USFM importer keeps).
Found and `scripture_ref` targets are read in the Resource's own numbering and go through its
Verse map, like a reference in its text: Hebrew-numbered "Ps 3:9" is the Standard's Ps 3:8. A
found reference to a book the Resource does not have (an Old Testament verse in a New Testament's
note, or any reference in a Resource without Verses) has no own numbering and is read in the
Standard. The choice is made per item of a reference list, not per list: the finder reads the
text in the Resource's numbering for its books and the Standard for the rest (`OwnElseStandard`),
so "Ps 3:8; Rom 3:4; Ps 3:9" in a Hebrew Psalter is one link to Standard Ps 3:7, Rom 3:4 and
Ps 3:8. A link that wraps onto a second line is one widget per line, so it never covers the
links beside it. Hovering (or focusing) a link shows the reference popup
with the ★ Bible's text; clicking it goes there (`reference_navigate`, as a reference in the text
does) and closes the popover.

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
  pointer's), `focused_footnote` (the keyboard's), `clicked_footnote` and `footnote_by_keyboard`
  (that click was not the pointer's: `Response::clicked_by(Primary)` is false). A `FootnoteHit` names its
  Resource.
- `shell/footnotes.rs` owns the dwell, pinning and drawing; the popover's text, × and links
  have AccessKit names.

## Evidence

- `crates/ui-egui/src/footnotes_tests.rs` on BSB Isaiah 40:1-3 and John 1:1-23 from the official
  USFM (`crates/formats/tests/assets/usfm/bsb/`).
- `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui footnote_previews` writes the harness images under
  `target/previews/`; `docs/previews/footnotes-*.png` are those, converted.
- `cargo xtask bench`: the synthetic Bible ends every sixth verse with a Footnote marker.
