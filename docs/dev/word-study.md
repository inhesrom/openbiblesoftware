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
  straight to it.
- **Chooser** (`Session::word_chooser`, `Chooser`): the first Strong's-tagged English Bible
  in Preferred order whose rendering index has the words. Each number with its count, most
  frequent first, and the first Lexicon's Lemma and gloss. Words match whole normalized words
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
| `strongs_totals` | `strongs`, `count` (Tagged-word parts), `part_of_speech` (from readable Morphology) |

Hebrew and Greek words are not renderings, so TAHOT and TAGNT get totals only. The index is
derived, outside the content hash, like the transliteration index.

**Older files.** `FORMAT_VERSION` stays 3: a file written before #55 has no such tables, still
opens, and `ResourceStore::rendering_index` answers `None`. The engine then derives the index
from the Bible's text once per revision and keeps it in memory
(`word_study::RenderingIndexes`). Bumping the version would have refused every installed
Starter `.obr` until a rebuild; the next Starter build carries the tables.

## Starter Preferred order

`STARTER_PREFERRED_ORDER` (engine `library`) ranks Starter Resources the user has not
ranked: `kjv-strongs` first among Bibles (the tagged English Bible decision #15 names), and
Lexicons TBESH, TBESG, BDB, Abbott-Smith, Strong's Hebrew, Strong's Greek. Without it the
library's file order put ASV first (its eBible tagging is sparse: "loved" in John 3:16 is
untagged) and BDB before TBESH. A saved Preferred order always wins.

## Evidence

- Engine `word_study_tests`: the box; "love" offers G25 ×3, G26 ×2, G5368 ×2 from the KJV
  fixture; one-number words go straight there; H1254A lists TBESH, BDB, then Strong's (base
  H1254) with the Starter Lexicon excerpts scanned in file order; the header; Word Info and
  `word_study.open`; Link sets; history; the Last session.
- Model `renderings::tests`, store `rendering_index_round_trips_and_older_files_answer_none`.
- UI `shell::tests::word_study`: typing "love" in the box shows the chooser rows with counts
  and choosing G26 studies it; H1254A's entries in order with only the first expanded; Word
  Info's button, a Strong's link in an entry and one in a Lexicon Tab open the Word Study.
- Starter library (manual, `cargo test --release -p obs-engine --lib
  starter_library_love_and_h1254a -- --ignored --nocapture`): "love" in the KJV offers
  G26 ×86, H157 ×74, G25 ×73, H160 ×32, G5368 ×10 and others; H1254A lists TBESH, BDB,
  Strong's; header bara, "to create" (TBESH), verb, 53 occurrences in TAHOT. The first chooser
  takes ~0.5 s (deriving the KJV index from a pre-#55 `.obr`), later ones ~7 ms; Lexicon
  entries ~5 ms.
