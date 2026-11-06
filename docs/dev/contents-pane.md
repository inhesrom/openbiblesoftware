# Contents pane (#176)

Each Resource Tab's header has a **Contents** button (the outline icon at its left, beside the
Link-set chip). It shows the Resource's **Contents** in a pane beside the text:

| Resource type | Outline |
|---|---|
| Bible | Bible books → chapters, in the Bible's own order and numbering |
| Commentary | volumes (when the Commentary names them) → Bible books → chapters → sections; a book's or chapter's introduction comes first |
| Lexicon, Dictionary | entries in the Resource's own order, under the first letter of their headword or Lemma (A–Z, Α–Ω, א–ת), with a **Filter A–Z** box |

The line of the Tab's Current location is highlighted and its parents opened; as the text
scrolls (or a Link set moves it) the highlight follows, and the list scrolls to the line only
when it is out of view. Clicking a line takes the Tab there; clicking a group line (volume, book,
chapter, letter) also opens it and goes to its first entry. The triangle opens or closes a line
without moving the Tab.

## Contract

- Engine `obs_engine::contents`: `Contents::build(type, verses, units, navigation, volumes)`
  makes the outline from what the `.obr` already holds: the Bible verse list, the Keys
  (`ResourceNavigation`), unit titles (a Lexicon's Lemma, a Dictionary's headword) and a
  Commentary's volumes. `at_verse` (Bible chapter line of a Verse in own numbering), `at_unit`
  (section or entry line of the unit at the view's top), `ancestors`, `filter`.
- `contents::go(workspace, tab, target)` is the jump: a chapter is `Workspace::navigate` to its
  first Verse; a section or entry is `navigate` / `navigate_entry` by its Key. When that Key would
  land on another unit (MHC's book introduction starts at Gen 1:1, where the narrower Gen 1:1-2
  section wins) it is `navigate_text` to the unit itself. Every one is a Back/Forward step and a
  Visit, and the Link set follows.
- `tab.contents {tab, on?}` shows, hides or toggles the pane of a Resource Tab; other Tabs are
  refused. The state is `Tab::contents`, kept in Layouts and the Last session as an optional
  `TabSnapshot.contents` (absent when off, so older snapshots read unchanged).
- UI `shell/contents_pane.rs`: `ContentsCache` builds each Resource's Contents once, on a worker
  (`JobQueue::threaded(1)`; inline on wasm and in single-threaded tests), the first time a pane of
  it opens, and shares it between that Resource's Tabs. A new revision (a new `ReadingDoc`) is
  built again. The list draws only the rows in view (`ScrollArea::show_rows`, fixed-height rows).

## `.obr`: `volumes`

MHC is printed in six volumes, and the Wikisource page titles say which
(`…/Volume 1/Genesis/Chapter 1`). The MHC importer now records them (`ResourceBuilder::set_volume`)
into a new `.obr` table `volumes (book, title)`. It is navigation metadata outside the content
hash, so MHC's content hash, Annotations and Search indexes are unchanged. A file without the
table (any `.obr` written before) opens with no volumes; its Contents start at the Bible books.
The `.obr` layout table in `crates/store/src/obr.rs` documents it.

## Decisions

- **No format version bump.** The ticket allows a schema version bump for a new index, but the
  reader refuses every file whose version differs, so a bump would make every installed library
  unreadable until rebuilt. `volumes` follows the precedent of `renderings` (#55) and
  `entity_strongs` (#149): an optional table, tolerated when absent, with no version change.
- **Commentaries get a chapter level** (volume → book → chapter → section). The ticket says
  volume → book → section, but JFB has 19,774 sections (Genesis alone over 1,000), so a flat book
  would not be a sensible tree. Sections are labelled by verses ("Verses 1–2", "Verse 4"); a
  section across chapters keeps both ("116:10–117:1"). MHC's `.book-intro` and `.chapter-intro`
  units (and a SWORD Commentary's whole-book key) are "Introduction"; a whole chapter otherwise is
  "Whole chapter".
- **Lexicons are A–Z by Lemma.** A Lexicon line is its Lemma and Strong's number
  ("ἀγάπη (G26)") under the Lemma's first letter, in the Lexicon's own order (Strong's numbers are
  themselves in Lemma order). The filter matches the label with case and diacritics ignored
  (`obs_model::normalize`): entries starting with the text first, then those containing it.
  Enter in the box opens the first match.
- **The button is an icon.** A text button made the reference box collapse in ordinary Panel
  widths; the icon sits left like a sidebar toggle, says "Contents" in its tooltip and AccessKit
  label, and is filled while the pane shows. The pane is on the left, about a third of the Tab
  (170–280 points, never over half).

## Evidence

- Engine `contents_tests`: Bible books/chapters and Psalm-title chapter start; Commentary volumes,
  introductions first, chapters and labels; without volumes; Dictionary A–Z and the filter;
  Lexicon Lemma + Strong's labels; `go` for a chapter, a section, the book introduction by unit
  (and Back), an entry with its Link set; `tab.contents` toggling and Layout round trip.
- `real_starter_contents_of_kjv_mhc_and_easton` (ignored; `OBS_REAL_STARTER=<dist/starter>`) on
  the real Starter build: KJV 66 books / 1,189 chapters; MHC's six volumes "Volume 1
  (Genesis–Deuteronomy)" … "Volume 6 (Acts–Revelation)"; Easton A–Z, and "Abraham" first for the
  filter "Abraham" in well under a frame (68 µs; built in 7 ms, MHC in 4 ms, KJV in 1 ms).
- UI `contents_pane` tests (real pointer and key events): the button, highlight painted, chapter
  click and Link set, the highlight following wheel scrolling, collapse; Commentary volumes and
  the book introduction; the Dictionary filter typed and clicked, and Enter; the list scrolling
  to a far entry and staying put for a near one; the worker build shared by two Tabs.
- Store `commentary_volumes_round_trip_outside_the_content_hash_and_older_files_have_none`;
  formats `mhc_sections_intro_markup_and_notes` (volume from the page title).
- Previews: real app on macOS (`OBS_SCRIPT` + `OBS_SHOT`, Starter KJV, MHC and Easton):
  [KJV and MHC at Ps 23](../previews/contents-app-kjv-mhc-ps-23.png),
  [Easton at Abraham](../previews/contents-app-easton-abraham.png); shell harness
  (`UPDATE_PREVIEWS=1 OBS_REAL_STARTER=… cargo test -p obs-ui-egui contents_pane_previews`):
  [MHC volumes](../previews/contents-mhc-volumes.png),
  [Easton filter "Abraham"](../previews/contents-easton-abraham.png).
