# Word Study: header, Lexicon entries and the English-word chooser (#55)

Decisions #15 (Word Study sections, Guide arrangement) and #20 rule 5 (derived indexes).
Translated as, Occurrences and Forms are #56. The Guide framework is #53
([passage-guide.md](passage-guide.md)); the Word Study uses it unchanged.

## Contract

- `obs_engine::word_study` owns the Word Study. Its Key is a Strong's number
  (`Key::StrongsNumber`), held in the Tab's entry history, so Back/Forward, Link sets and
  the Last session work as for a Lexicon Tab.
- **Box.** `tab.go` on a Word Study reads a Strong's number (`G5485`, `H1254`, `H1254A`;
  `word_study::parse_input`) and moves the Guide and its Link set. Anything else is English
  words: the Tab shows the chooser (`WordStudies::choosing`) until a number is chosen, Back /
  Forward or Cancel / Escape. When exactly one number renders the words, the Guide goes
  straight to it, whether the chooser job just finished (`poll_word_studies`) or its answer
  was already cached (`choose_words`).
- **Chooser** (`Session::word_chooser`, `Chooser`): the Strong's-tagged English Bibles
  ranked by how many Strong's numbers they render (Preferred order breaks ties); the first
  whose rendering index has the words answers. Each number with its count, most frequent
  first, and the first Lexicon's Lemma and gloss. Ranking by numbers, not by Preferred order
  alone, keeps a poorly tagged Bible the user ranks ★ (the Starter eBible ASV) from
  answering; ranking by Tagged words would not, since the eBible ASV and WEB tag about twice
  as many words as the KJV, on fewer numbers (no G25 or G26; "the" is H3605 10,000 times). Words match whole normalized words
  anywhere in a rendering ("love" counts "the love" but not "loved").
- **Header** (`Session::word_study_header`, `WordStudyHeader`), always on top, not a section:
  Lemma (the first Lexicon entry's headword), transliteration (`obs_model::transliterate`),
  Strong's number, short Gloss and its Lexicon, part of speech, total occurrences. The count
  comes from the first Bible in Preferred order in the number's original language that tags
  it (TAHOT for H, TAGNT for G), else the first tagged Bible that does; an extended number
  falls back to its base number in a Bible that tags only base numbers ("1 occurrence in KJV
  (as H1254)"). The part of speech is the commonest one the Bibles' Morphology gives.
- **Lexicon entries** (`GuideSection::LexiconEntries`, id `lexicon_entries`): every Lexicon
  in Preferred order that has the number, through `ResourceStore::lookup_strongs` (extended
  number first, then the base number). A base number lists every homograph a Lexicon keys
  under it. The first entry is expanded, the rest collapsed; expanding is per Tab and resets
  when the number changes. Each entry has Open in <Lexicon> and its Strong's links.
- The header and chooser are jobs on the Session job queue like Guide sections
  (`poll_word_studies` is chained in `poll_imports`); asking never waits.
- **Ways in:** Word Info's Word Study › (`word_info.word_study`), `word_study.open {strongs,
  new_tab?}`, Strong's links in a Lexicon entry (in the section and in a Lexicon Tab, where
  they are now drawn as links), the Guides menu (starts at the Link set's number, else the
  word Word Info shows) and the box. A plain click moves the focused or most recent Word Study;
  Ctrl/Cmd-click opens a new one.

## The rendering index (#20 rule 5)

`obs_model::RenderingIndex` is built by `ResourceBuilder::build` for every Bible, so Import
and the Starter build both produce it, and is written to two new `.obr` tables:

| table | rows |
|---|---|
| `renderings` | `rendering` (a Tagged word's normalized words, `the world`), `strongs`, `count` |
| `strongs_totals` | `strongs`, `count` (Tagged-word parts), `part_of_speech` (from readable Morphology; a stable id, `PartOfSpeech::id`) |

Hebrew and Greek words are not renderings, so TAHOT and TAGNT get totals only. The index is
derived, outside the content hash, like the transliteration index. `part_of_speech` holds a
stable id (`noun`, `foreign`, …), not the display name (`transliterated word`), so a label can
be reworded; an id the reader does not know reads as no part of speech, never as a damaged
file (which would also fail `ObrReader::load` and the Search index build).

**Older files.** `FORMAT_VERSION` stays 3: a file written before #55 has no such tables, still
opens, and `ResourceStore::rendering_index` answers `None`. `ObrReader::load` then leaves
`Resource::renderings` empty: Search index builds load every Bible and never use the index.
Word Study derives it from the Bible's text only when it needs it, once per revision, under
a per-Bible lock so two jobs never derive the same index at once, and keeps it in memory
(`word_study::RenderingIndexes`). Bumping the version would have refused every installed
Starter `.obr` until a rebuild; the next Starter build carries the tables. Until then the
first chooser, or a header whose number has no part of speech in the original-language
Bibles, derives every tagged Bible's index (2–5 s in release on a job, not the UI thread);
later ones read it from memory.

## Starter Preferred order

`STARTER_PREFERRED_ORDER` (engine `library`) ranks Starter Resources the user has not
ranked: `kjv-strongs` first among Bibles (the tagged English Bible decision #15 names), and
Lexicons TBESH, TBESG, BDB, Abbott-Smith, Strong's Hebrew, Strong's Greek. Without it the
library's file order put ASV first (its eBible tagging is sparse: "loved" in John 3:16 is
untagged) and BDB before TBESH. A saved Preferred order always wins.

**Existing installs (judgement call).** `attach_user` saves the Preferred order on every
launch, so an install that ran the app before #55 already has file order saved (ASV ★, BDB
before TBESH) and never sees the Starter order. It is not migrated: the saved order is the
user's ranking, and the app cannot tell a never-touched order from a deliberate one. The
chooser does not depend on it (it ranks tagged Bibles by numbers rendered), so "love" still
offers G25, G26 and G5368 with counts from the KJV. H1254A's Lexicon entries follow the
saved order there: BDB, Strong's, TBESH (all three, first expanded) until the user drags
TBESH up in the Library.

## Evidence

- Engine `word_study_tests`: the box; "love" offers G25 ×3, G26 ×2, G5368 ×2 from the KJV
  fixture; one-number words go straight there, also from a cached chooser; a poorly tagged
  Bible ranked ★ (more Tagged words, fewer numbers) does not answer "love" but still answers
  words only it renders; an index is derived once when four jobs ask at once; H1254A lists TBESH, BDB, then Strong's (base
  H1254) with the Starter Lexicon excerpts scanned in file order; the header; Word Info and
  `word_study.open`; Link sets; history; the Last session.
- Model `renderings::tests` (stable part-of-speech ids), store
  `rendering_index_round_trips_and_older_files_answer_none` (and `load` does not derive) and
  `part_of_speech_is_stored_as_a_stable_id_and_unknown_ids_read_as_none`.
- UI `shell::tests::word_study`: typing "love" in the box shows the chooser rows with counts
  and choosing G26 studies it; H1254A's entries in order with only the first expanded; Word
  Info's button, a Strong's link in an entry and one in a Lexicon Tab open the Word Study.
- Starter library (manual, `cargo test --release -p obs-engine --lib
  word_study_tests::starter_library -- --ignored --nocapture`): "love" in the KJV offers
  G26 ×86, H157 ×74, G25 ×73, H160 ×32, G5368 ×10 and others; H1254A lists TBESH, BDB,
  Strong's; header bara, "to create" (TBESH), verb, 53 occurrences in TAHOT.
  `starter_library_with_a_preferred_order_saved_before_55` attaches a User database whose
  saved order is file order (ASV ★): "love" still comes from the KJV with the same counts;
  H1254A lists BDB, Strong's, TBESH. With an imported NASB 1995 in the library (14,333
  numbers rendered against the KJV's 14,057) the chooser reads the NASB (G26 ×115, G25 ×77,
  G5368 ×13). The first chooser takes 2.7–4.5 s (deriving every tagged Bible's index from
  pre-#55 `.obr`s), later ones ~45 ms; Lexicon entries ~1–40 ms.
