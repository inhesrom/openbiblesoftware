# Exegetical Guide (#144)

A Guide whose Key is a Passage, read word by word in its original language. It reuses the
Guide framework ([passage-guide.md](passage-guide.md)): sections are engine data computed on
the job queue, and the Guide arrangement is per kind of Guide (`exegetical_guide`).

## Contract

- **Key.** `Key::Passage` in the Standard versification, kept in the Tab's history like the
  Passage Guide's. `TabKind::Guide(Guide::ExegeticalGuide)` follows `KeyScheme::Passage`, has
  a reference box and a link chip, and joins Link sets. `guide.exegetical_guide` takes the
  same `passage` / `new_tab` / `split` params as `guide.passage_guide`; without `passage` it
  starts at its Link set's Passage or, unlinked, at the focused Tab's Verse. Opened from the
  Guides menu, a Verse's context menu, and the Command box (`eg Jn 3:16`,
  `exegetical guide Ps 23`). Favorites and History store its Passage like the Passage Guide's.
- **Which Bible** (`exegetical_guide::library_word_by_word`). For each Standard Verse of the
  Passage, the first Bible in Preferred order in that Verse's original language (Hebrew or
  Aramaic for the Old Testament, Greek for the New, by manifest language:
  `key_words::is_original_language`) whose own Verses for it hold Tagged words. The Standard
  Verse goes to the Bible's own numbering through its Verse map (`VerseMap::to_native`), so
  English Ps 3:1 reads TAHOT's Ps 3:2 and the Psalm title slot (Ps 3:title) reads TAHOT's
  Ps 3:1. A Bible Verse reached from two Standard Verses is listed once. Each verse block
  carries its Standard Verse and its own (`ExegeticalVerse::renumbered`), and the UI says
  "Ps 3:1 in English Bibles" under a renumbered one.
- **Word by word** (`SectionData::WordByWord`). Every `tagged_word` span of the Verse in text
  order: the word as written and its transliteration, the main part's Strong's number and
  Lemma (the first part below STEPBible's affix numbers H9001/G9001, as Word Info picks it),
  the Gloss of all parts, and Morphology in plain English (`Morph::long` of each part's code,
  joined with ` + `; raw codes only in the hover). A click opens a Word Study at the main
  number, reusing an open Word Study Tab (Ctrl/Cmd: a new one). The header counts words.
- **Lexicon summaries** (`SectionData::LexiconSummaries`). Each distinct main Strong's number
  in order of first use, with how often the Passage uses it, and the first Lexicon in
  Preferred order that has it (extended number, then base; an entry opening with an editorial
  note `[` / `(` gives way to the next Lexicon, as in Key words). The short entry is the
  entry's paragraphs after its headword line, whitespace collapsed, cut at a word to 320
  characters (`short_entry`). The Strong's number opens a Word Study; the Lexicon's
  abbreviation opens the Lexicon at the entry. First 12 with Show all.
- **Grammatical notes** (`SectionData::GrammaticalNotes`). Each Morphology term the words
  use, in order of first use, with the words that use it. Terms come from
  `obs_model::Morph::terms`: stem, tense (Hebrew/Aramaic "verb form"), voice, mood, non-finite
  form, case, noun state, and a dual number. `GrammarTerm::explanation` is our own plain
  English, per language where a name means different things (the Hebrew and Greek perfect,
  imperfect, imperative and participle). Every term TAHOT and TAGNT use has its own note
  (`crates/model/tests/morph.rs`); only the rarer Hebrew and Aramaic stems share the general
  note on stems.
- **Empty states** (#135). Word by word and Grammatical notes need a Hebrew or Greek Bible
  with Morphology in the Passage's language (`missing_resource::key_language`: Hebrew for the
  Old Testament, Greek for the New): Get Hebrew & Greek texts…, and the Starter library when
  its TAHOT or TAGNT is missing (TAGNT for John even with TAHOT in the Library). Lexicon
  summaries need a Lexicon: Get Lexicons….
- All three sections read the words separately (each is its own job); a verse's words are a
  few indexed reads, so nothing is cached beyond the Bibles' Verse maps (`BibleCache`).

## Not here

Textual variants and apparatus, syntax and grammars: there is no open data for them (#144
non-goals).

## Tests

- `crates/engine/src/exegetical_guide_tests.rs`: TAGNT Jn 3:16 word by word (Lemma,
  Morphology, Gloss in order), English Ps 3:1 → TAHOT Ps 3:2 through the Verse map, the title
  slot and the whole chapter, Lexicon summaries, Grammatical notes (Greek and Hebrew), the
  empty state, the command and Link set, the menu and Command box; an ignored check against
  the real Starter library (`cargo test -p obs-engine --lib starter_library_exegetical --
  --ignored --nocapture`).
- `crates/model/tests/morph.rs`: every term of every TAHOT/TAGNT code has a note.
- `crates/ui-egui/src/guide_tests.rs`: the Guide beside a Bible in the shell harness, a word
  opening its Word Study, the Link set, and TAHOT's Ps 3:2 at English Ps 3:1.
