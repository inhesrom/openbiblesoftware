# Factbook (#149)

A Guide whose Key is one person, people group or place of a People & places index (TIPNR in
the Starter library). It reuses the Guide framework ([passage-guide.md](passage-guide.md)):
sections are engine data computed on the job queue, the Guide arrangement is per kind of Guide.

## Contract

- **Key.** `Key::Headword(<entity id>)` in the Tab's entry history, e.g.
  `Abraham@Gen.11.26-1Pe` (TIPNR's unique name). `TabKind::Guide(Guide::Factbook)`,
  `HeaderBox::Name`; no link chip, no Key schemes: a Factbook never follows or moves a Link
  set. Back/Forward walk the entry history.
- **People & places directory.** `factbook::PeopleDirectory`: every entity of the library's
  People & places indexes in Preferred order (an id two indexes list is kept once), with its
  mention count and first/last Verse (`ResourceStore::mention_stats`, one SQL pass). It is
  loaded once per library (ids, revisions, hashes, paths) by `DirectoryCache`, shared by the
  Session and the section jobs. `Session::people_directory()` never waits: Loading starts a
  job, `poll_imports` (chaining `poll_factbooks`) finishes it. TIPNR: 4,135 entities in ~20 ms.
- **Telling people apart** (`PeopleDirectory::distinction`), structured fields only: a named
  parent (`son of Zebedee`, `daughter of …`), else a named child (`father of Peter`), spouse,
  sibling or forebear, else the region; places use their region. TIPNR writes unnamed
  relatives in lower case (`father_of_Lazarus`); they are skipped. TIPNR's descriptions are
  never stored (#43), so nothing here can show them.
- **Chooser.** `tab.go` on a Factbook (the header box) takes a name: the Tab shows the chooser
  for it (`Factbooks::choosing`), or moves straight to the only entity with that name. An
  entity id (contains `@`) moves the Tab. `PeopleDirectory::choose` matches the normalized
  name against usual and other names (`_` as space): entities called that by usual name first,
  then by another name (`Mark`, also called `John`), each group most often named first; with no
  exact match, names that start with the text.
- **Sections** (`GuideSection`, ids stored, never renamed): `summary` (other names, the Name
  meaning, family: Father, Mother, Descended from, Siblings, Married to, Children, Founded by),
  `key_passages` (every mention, neighbouring Verses joined, Bible order), `dictionary_articles`
  (each Dictionary's article in Preferred order, through the #54 alias cache, with ~600
  characters of it), `related` (the entity's other links, the entities whose records link to
  it, then up to 10 named in the same Verses). Map is the Atlas's (#150).
- **Name meaning.** The first Dictionary in `factbook::NAME_MEANING_DICTIONARIES`
  (`hitchcock`) with an article on the entity, through the alias cache; the headword is
  dropped, and two senses of one name are joined with ` · `. That Dictionary is left out of
  Dictionary articles.
- **Ways in.** `guide.factbook {entity | name | strongs, new_tab?, panel?, split?}`; the
  Guides menu; a People & places row's name in the Passage Guide; Word Info's `Factbook: …`
  button when the word's Strong's number names people or places (the exact extended number,
  else every entity under the base number: the KJV's G2491 opens the John chooser).
  Favorites and History label a Factbook `John (son of Zebedee) · Factbook`.

## Starter library: Hitchcock's Bible Names

- Lock entry `hitchcock` (importer `sword-zip`): CrossWire's `rawzip/Hitchcock.zip`, pinned by
  sha256, evidence in `starter/evidence/hitchcock.md` with the unmodified conf. Public domain;
  CrossWire-hosted like Easton's, no CCEL-hosted file.
- `sword-zip` unpacks a CrossWire package to a scratch folder and imports its one module
  through the app's own SWORD Import (`obs_formats::import_sword`), as Library → Import would.
- SWORD Import now combines a headword a module lists twice into one entry (two paragraphs)
  instead of dropping the second; Hitchcock lists Abel, Eli, Noah and Rahab twice.
- 2,612 entries, 0.5 MB plus its Search index; the alias cache links 3,406 of TIPNR's 4,135
  entities to a Hitchcock entry.

## Evidence

- Engine `factbook_tests`: Abraham from the Passage Guide row (family, meaning, Key Passages,
  articles, History label); the John chooser; Word Info Strong's numbers; Related both ways
  and named together; neighbouring Verses; meanings; empty library and reload.
- UI `shell::tests::guide::abram_opens_his_factbook_and_the_john_chooser_lists_the_johns`.
- Real Starter library (manual): `OBS_LIBRARY=dist/starter cargo test -p obs-engine
  starter_library_abraham_factbook -- --ignored --nocapture`.
