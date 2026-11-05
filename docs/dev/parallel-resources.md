# Parallel resources (#151)

A Resource Tab's header has a parallel-resources control, right of the title:
◀, a dropdown of every Resource of the Tab's Resource type in Preferred order, and ▶.
Ctrl+[ and Ctrl+] are ◀ and ▶. Choosing one shows that Resource in the same Tab at the
same place.

## Contract

- Engine commands: `tab.parallel {tab, id}` (the dropdown), `tab.parallel_previous {tab}`
  (Ctrl+[) and `tab.parallel_next {tab}` (Ctrl+]). Previous/next wrap around the Preferred
  order. `parallel_resources(session, tab)` lists the dropdown's Resources. The keys are
  enabled only while the focused Tab is a Resource Tab, so they stay free elsewhere.
- `Workspace::switch_resource` changes the Tab's `TabKind::Resource`, drops what was
  loaded from the old Resource (Verse map, numbering, sections) and queues
  `Effect::Switched(tab)`. The command layer sets the Tab's title and Key schemes from the
  library, so the Tab bar, header box and Last session follow at once.
- A Bible or Commentary Tab records its Current location as a Standard Verse
  (`Tab::arriving_at`, through the old Bible's Verse map). When the UI has loaded the new
  Resource it calls `Workspace::arrival`, which translates that Verse into the new
  Resource's numbering (`link::to_native`), or for a Commentary picks the nearest earlier
  section, exactly as a Link-set follower does. The landing Verse is flashed.
- A Lexicon or Dictionary Tab keeps its entry Key; the new Resource looks it up (exact,
  then the base Strong's number). Without it, the Tab opens where a new Tab of that
  Resource would: the Link set's entry, else the first entry.
- `History` (both the Verse and the entry-Key history) tags each Back/Forward entry with
  its Resource once the Tab has switched. The switch itself is one history step: Back
  restores the old Resource and its location in its own numbering, Forward replays the
  switch. Ordinary jumps before and after keep working as before.
- The Tab keeps its scroll anchor across a switch. The UI parks where the old view was
  scrolled (`TabUi::parked`: Resource, location in its own numbering or Key, and a
  `view::TextAnchor` — the top line's unit and byte plus how far into that line) and lays
  the new Resource out at the same width. When Back or Forward returns to a parked place
  (the Tab's history location in that Resource matches), the view is scrolled to the
  anchor rather than to the location's start, so a reader halfway through a long
  Commentary section is halfway through it again. A fresh switch (▶, ◀, dropdown) still
  lands on the mapped place and flashes it.
- The header box shrinks to leave room for the controls on its right (measured the frame
  before), and the dropdown is narrower in a narrow Panel, so nothing overlaps.
- The Tab keeps its id, Panel, Link set and history. A switch, and Back/Forward across
  one, does not move the Link set: the place is the same in Standard terms, and a
  fallback (nearest earlier section, Absent verse) is never sent back to the set.
- In the frame the switch is clicked, the Tab's view still holds the old Resource until
  the effect is applied; `reading_body` skips such a stale view so it cannot report the
  old Resource's location as the new one's.

## Decisions

- The control is on every Resource Tab, Bibles, Commentaries, Lexicons and Dictionaries
  alike. A type with one Resource shows it disabled, saying there are no others.
- Cycling wraps around (the issue says "cycle"). The dropdown lists the Tab's own Resource
  too, checked, so its order shows where ◀ ▶ go.
- A Commentary lacking the Passage shows its nearest earlier section (the issue allows
  either; it is what Link sets already do). One with no section at or before the Passage
  opens at its start, as a new Tab of it would; Back still returns.
- An Unmapped verse has no Standard place, so the new Bible opens where a new Tab would
  (its Link set's location, else where another Bible is).
- History is not saved in the Last session (as before); the switched-to Resource and its
  location are.
- Parked scroll anchors are UI state keyed by (Resource, location), at most 64 per Tab,
  not engine history: the engine has no layout to anchor to. Two stops at the same place
  in the same Resource share the latest anchor; the place itself is always right.
- In a narrow Panel the reference box gives way to the controls (down to 48 points, its
  text scrolling inside) rather than wrapping the controls onto a second row.

## Evidence

- Engine `parallel_resources_tests`: BSB Ps 3:1 → TAHOT Hebrew Ps 3:2 and Back/Forward;
  the switch as one history step among jumps; Preferred order, wrapping, type filter and
  key enabling; Commentary nearest earlier section; Lexicon Key; Last session.
  The BSB fixture is the BSB download's own USFM for Ps 3:1-3 (`\d \v 1`), imported with
  the Starter's USFM importer; TAHOT is the STEPBible Job–Song test excerpt imported with
  the Starter's TAHOT importer.
- `real_bsb_ps_3_1_switches_to_real_tahot_ps_3_2` (ignored by default) runs the same switch
  on the real pinned sources: `bsb_usfm.zip` (sha256 `f0c56438…5f87`) `PSA.usfm` and the
  TAHOT Job–Song file (sha256 `e0416880…72e8`).
- UI `parallel_resources` (real pointer and key events in the shell harness): ▶ lands on
  TAHOT Ps 3:2 and flashes it, ‹ returns to BSB Ps 3:1, › goes forward again; Ctrl+] and
  Ctrl+[ cycle; the dropdown lists the Bibles and switches; a lone Commentary's control is
  disabled; Back across a switch returns to the same scroll position inside a long
  Commentary section (line, unit byte and offset into the line, and the same text
  painted), and in a narrow Panel the reference box ends left of ◀.
- Snapshots: [BSB at Ps 3:1](../previews/parallel-resources-bsb-ps-3-1.png),
  [dropdown](../previews/parallel-resources-dropdown.png),
  [TAHOT at Ps 3:2](../previews/parallel-resources-tahot-ps-3-2.png),
  [Back to BSB](../previews/parallel-resources-back-bsb-ps-3-1.png), written by
  `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui parallel_resources_previews` and encoded
  with ffmpeg.
