# Guide framework and Passage Guide (#53)

Decisions #15 (sections, Guide arrangement, navigation) and #18 (Tab header, placement).
Key words and People & places are #54; the Word Study Guide is #55 (still a placeholder).

## Contract

- `obs_engine::guide` owns the framework. `GuideSection` is the engine type for one
  section (`Commentaries`, `CrossReferences`); `Guide::sections()` lists a Guide's sections
  in default order. Section and Guide ids (`commentaries`, `passage_guide`, ...) are stored
  and passed to commands; never rename them.
- `Session::guide_section(&key, section)` never waits. A section that is not computed yet is
  submitted to the Session job queue (`jobs().submit_fn`) and reported as `Loading`; ask
  again next frame. Each section is its own job, so a slow section never holds back another.
  `Session::poll_imports` applies finished section jobs (it chains `poll_guides`) and still
  returns unrelated events. Pending sections nobody asked for since the last poll are
  cancelled; a library change (ids, revisions, hashes, paths, Preferred order) drops every
  result.
- `SectionData` is pure data: `Commentaries(Vec<CommentaryRow>)` and
  `CrossReferences(Vec<ReferenceGroup>)`. `ui-egui` draws it. `SectionSource` computes it;
  `LibrarySections` reads the library's `.obr` files and is the default. Tests install
  their own source to hold a section back (`Guides::set_source`).
- Commentaries: one row per Commentary in Preferred order. The section chosen is the one the
  Commentary's own Tab shows for the Passage's first Verse (`ResourceNavigation::section`,
  #102) when it overlaps the Passage; otherwise the first section starting inside the
  Passage; otherwise the latest, narrowest section still overlapping it. The row carries the
  section's Passage, its leading Heading span (if any) and up to 900 characters of the text
  after it. Each Commentary's Passage Keys are loaded once per revision and cached.
- Cross-references: `cross_references::library_references` (#51), unchanged. The Guide
  draws them with the Tool's `reference_groups`: grouped by source Verse, top 8 with Show
  all, worker-backed one-line previews and hover popups.
- `GuideSection::empty_hint(&library)` is the empty state: "None in your library. Add a
  Commentary …" when the library has no Resource of the needed type, otherwise "None in
  your library: no Commentary has a section on this Passage."

## Guide arrangement

- `GuideArrangement` (order, collapsed, hidden) is per kind of Guide, kept in
  `Session::guides`, and saved as the User database preference
  `guide-arrangement/<guide id>` on every change. Loading tolerates unknown, repeated and
  missing section ids; sections a later ticket adds appear at their default place.
- Commands: `guide.move_section {guide, section, to}`, `guide.collapse_section {guide,
  section, collapsed?}` (toggles when `collapsed` is missing), `guide.hide_section {guide,
  section, hidden?}` and `guide.reset_arrangement {guide}`.
- UI: click a header to collapse or expand it, drag it to move it (a lapis line marks the
  drop gap; Escape cancels), × hides it. Hidden sections are listed under the last section
  with a Show button, and in the Tab's ⋯ menu as checkboxes beside Reset arrangement.

## Navigation

- The header box is the usual reference box: `tab.go` reads a Passage (a range such as
  `Jn 3:16-18` is kept until the Guide moves to another first Verse). The Guide joins Link
  sets by its Passage Key scheme. Opened while unlinked, it starts at the Passage of the
  focused Tab.
- Cross-reference rows: hover shows the Scripture popup; click navigates the Guide's Link
  set; Ctrl/Cmd-click opens the ★ Bible in a new Tab (#51 rules).
- Commentary rows: hover shows a popup with more of the section; click opens the Commentary
  at that section (its open Tab if there is one, otherwise a new Tab in the usual place,
  joining the Guide's Link set), without moving the Guide or its Link set; Ctrl/Cmd-click
  always opens a new Tab.

## Evidence

- Engine `guide_tests`: job-queue contract, held section independence, rows in Preferred
  order with headings, section choice, empty hints, library changes, cancellation,
  arrangement commands and persistence across restarts, tolerant loading.
- UI `shell::tests::guide`: sections appear as they finish while a held Commentaries job
  keeps frames under 500 ms; arrangement changes apply to every Passage Guide and survive a
  restart; the no-Commentaries hint; navigation rules; typing a range in the Guide box.
- Starter library (manual, `cargo test -p obs-engine --release
  starter_library_passage_guide -- --ignored --nocapture`): John 3:16 gives JFB (John 3:16)
  and MHC (John 3:1-21, "Christ's Interview with Nicodemus.") and 23 Cross-references.
  Commentaries take ~340 ms cold (loading both Commentaries' Keys) and under 1 ms warm.
