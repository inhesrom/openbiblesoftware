# Guide framework and Passage Guide (#53, #54)

Decisions #15 (sections, Guide arrangement, navigation), #18 (Tab header, placement) and #20
(derived indexes). Key words and People & places are #54 (see below); the Word Study Guide is
#55 (still a placeholder).

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
- `SectionData` is pure data: `Commentaries(Vec<CommentaryRow>)`,
  `CrossReferences(Vec<ReferenceGroup>)`, `KeyWords(KeyWords)` and
  `PeopleAndPlaces(Vec<PersonOrPlace>)`. `ui-egui` draws it. `SectionSource` computes it;
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

## Key words (#54)

- `obs_engine::key_words`. The ★ Strong's-tagged Bible is the first Bible in Preferred order
  with Tagged words in the Passage (a Bible without the Passage is passed over). Rows are its
  distinct Strong's numbers: Lemma, transliteration, Gloss, Part of speech, count in the
  Passage and total in that Bible, sorted by total (rarest first), then count, then number.
- Words without Lemma or Morphology (KJV+Strong's) borrow them from the same number in the
  same Passage of an original-language Bible (Greek for G, Hebrew/Aramaic for H: TAGNT,
  TAHOT), found by the manifest language. The Gloss is the first Lexicon in Preferred order
  that has the number (an entry opening with an editorial note, Abbott-Smith's `[in LXX …]`,
  is passed over), else the Tagged word's own; the Lemma falls back to that Lexicon's
  headword.
- Function words are left out by Part of speech (`PartOfSpeech::is_function_word`: articles,
  conjunctions, prepositions, particles, pronouns, interjections, Hebrew suffixes), as are
  STEPBible's affix numbers (H9001 and up). With no Morphology anywhere nothing is filtered;
  rarest-first still sends common words to the end. The count left out is shown.
- Totals come from one scan of the Bible's `tagged_word` spans
  (`ResourceStore::for_each_tagged_word`, a single SQL pass), kept in memory per Bible
  revision (`BibleCache`, with the Verse maps).
- UI: one line per word (Lemma through the text pipeline, transliteration, Gloss, `1× · 146 in
  KJV`), the first 12 with Show all. Click opens a Word Study at the Strong's number: the open
  Word Study Tab moves there, or `guide.word_study {strongs}` opens one (that command now takes
  an optional `strongs` and puts it in the Tab's entry history, for #55); Ctrl/Cmd-click
  always opens a new one.

## People & places (#54)

- `obs_engine::people_places`. Mentions from every People & places index in Preferred order
  (an entity once). Name as the ★ Bible's text of the Passage gives it (`Abram` in Gen 12:1,
  whole word, case-sensitive) with the usual name beside it (`Abraham`); rows in the order the
  text names them, then by first mention. Facts are TIPNR's structured fields only: kind,
  region and relations (Father, Mother, Descended from, Siblings, Married to, Children,
  Founded, Founded by, Lived here). TIPNR's descriptions are never stored (#43) or shown.
- Article links: one per Dictionary in Preferred order with an article on the entity. Click
  opens that Dictionary at the headword (its open Tab moves there; Ctrl/Cmd-click: a new Tab).

### Dictionary alias cache (derived, #20)

- `obs_engine::aliases`: an `AliasTable` per (People & places index, Dictionary) pair maps
  entity ids to headwords. Matching: an override from `starter/aliases.toml` (compiled in),
  else the entity's names (usual name, `_` as space, then other names) against the
  Dictionary's headwords, their comma parts (`Pilate, Pontius`, `Sinai, Or Sinai`) and their
  aliases (`Abram` → `Abraham`), as written and then as plural/singular. Headwords compare
  after `obs_model::normalize`.
- The file `<index>.<hash16>--<dictionary>.<hash16>.v<ALIAS_VERSION>.aliases.json` names both
  content hashes; it also records the overrides digest. A table is used only if all of these
  match, so it is rebuilt whenever either Resource, the matcher version or the overrides
  change.
- Where: the app's cache folder (`aliases/` beside `search/`, or `$OBS_ALIAS_CACHE`), then the
  prebuilt `aliases/` beside the `.obr` files. `App::with_file` calls
  `Session::set_alias_cache_dir` and `Session::refresh_alias_caches` at library load: a
  background job makes every pair current and deletes cache-folder files no pair uses. A
  section job builds a missing table on demand too.
- `cargo xtask starter` writes the Starter's tables to `dist/starter/aliases/` and fails if an
  override names an entity or headword the Starter Resources lack.
- Starter coverage (real library): Easton links 3,148 and Smith 3,685 of TIPNR's 4,135
  entities; building both tables takes ~130 ms.

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
- #54 engine `guide_words_tests` (open fixtures: KJV and TAGNT-style John 3:16, KJV Gen 12,
  the TIPNR excerpt, Easton and Smith study fixtures): John 3:16 Key words lead with words
  found once and leave out γάρ, ὥστε, ἵνα, εἰς, αὐτός, μή, ἀλλά and the article; Gen 12:1
  lists Abram (Abraham) linked to Easton's and Smith's "Abraham"; alias cache rebuilt when a
  side changes, prebuilt tables used, overrides applied. UI `shell::tests::guide`: Key words
  rows, Show all, Word Study clicks; Abram's row and its Easton link.
- #54 Starter library (manual, `cargo test -p obs-engine --release starter_library_ --
  --ignored --nocapture`): John 3:16 from KJV+Strong's leads with μονογενής (9), αἰώνιος (71),
  ἀπολλύω (103) and ends with θεός (1,359), 7 function words left out; ~350 ms cold (Strong's
  totals and TAGNT's Verse map), ~4 ms warm. Gen 12:1 lists Abram (Abraham), Easton: Abraham,
  Smith: Abraham.
