# Topic Guide (#148)

A Guide whose Key is a topic (a Headword): the topical indexes' subtopics with their Passages,
the Dictionaries' articles, the most-cited Passages and a Library search. It reuses the Guide
framework ([passage-guide.md](passage-guide.md)); the topical index decision is
[ADR 0009](../adr/0009-a-topical-index-is-a-dictionary-whose-entries-are-passage-lists.md).

## Contract

- **Key.** `Key::Headword(<topic>)` in the Tab's entry history (`Forgiveness`).
  `TabKind::Guide(Guide::TopicGuide)`, `HeaderBox::Topic`, Key scheme Headword: it has a link
  chip and follows (and moves) its Link set's Dictionary headwords. Opened from a Bible it
  joins that Bible's Link set; opened blank in a Link set (from a Dictionary there, say) it
  starts at the set's current headword. Headwords compare normalized (`FORGIVENESS` = `Forgiveness`).
- **Topical index.** A Dictionary whose resource id is in `topic_guide::TOPICAL_INDEXES`
  (`nave`, `naves`, `torrey`). Every other Dictionary feeds Dictionary articles.
- **Topic directory.** `topic_guide::TopicDirectory`: every Dictionary headword (Headword Keys,
  aliases included) grouped by normalized headword, with its display form (the first spelling
  with lower-case letters, else `SIN, FORGIVENESS OF` → `Sin, Forgiveness of`) and its
  Dictionaries in Preferred order; plus the topical indexes' see-only entries (no Passage,
  only `See …`). Loaded once per library of Dictionaries by `TopicCache` on the job queue,
  shared by the Session and the section jobs, like the Factbook's directory. Real library
  (Nave, Torrey, Easton, Smith, ISBE): ~275 ms cold.
- **Reading an entry** (`topic_guide::subtopics`): one subtopic per line (paragraph). The label
  is the text before the line's first Bible reference; `→ ` and `- ` start a subtopic, `. `
  and a list paragraph (Nave's `<item>`) an item under one (level 2). `See X` (several per line)
  sends the reader to topic X. A line with no label adds its Passages and `See` to the line
  before (Torrey's reference lines, Nave's `See ENEMY` item). A subtopic with no Passage whose
  `See` names another entry's subtopic (`SIN, FORGIVENESS OF`) takes that subtopic's Passages
  (`via`).
- **Sections** (`GuideSection`, ids stored, never renamed):
  - `topical_entries` "Topical Bible entries": each topical index's entries filed under the
    topic, in Preferred order: the headword itself, then longer headwords that start with it
    as whole words (`FORGIVENESS OF INJURIES`, `PRAYER, ANSWERS TO`).
  - `topic_articles` "Dictionary articles": each other Dictionary's article under the topic's
    own headword, or when it has none, up to 3 under longer ones (Easton: `Forgiveness of sin`),
    with ~600 characters of it.
  - `topic_passages` "Key Passages": every distinct Passage the topical entries cite, ranked by
    how many citations share a Verse with it (narrower first, by Standard Verses spanned, then Bible order), skipping one
    that overlaps a Passage already chosen; at most 12.
  - `topic_search` "Search": the topic, for `search.run {text, mode: "library"}`.
- **Chooser.** `tab.go` on a Topic Guide (its box) takes text: the chooser lists the headword
  itself, longer headwords starting with it, then **synonyms** (see-only topical index entries
  whose `See` names the topic: Nave's `PARDON` → `See SIN, FORGIVENESS OF`, `IMPORTUNITY` →
  `See PRAYER`); with none of these, headwords starting with the text. Text naming one topic
  opens it directly. `guide.topic_guide {topic?, text?, new_tab?, panel?, split?}`: `topic`
  opens it as given, `text` chooses.
- **Ways in.** Guides menu and launcher (its box takes text), the Command box (`tg`, `topic`,
  `topic guide` + text), a `See` link or chooser row, Favorites and History (`Forgiveness ·
  Topic Guide`).
- **Empty states** (#135). Without a topical index, Topical Bible entries and Key Passages say
  to add Nave's or Torrey's and offer Get Dictionaries…, never the Starter library (it has
  none, ADR 0009).

## UI

- Topical Bible entries: per entry its title and an `Abbrev: HEADWORD` link that opens the
  Dictionary there; per subtopic its label (level 2 indented), its Passages as links (hover:
  the Scripture popup from the ★ Bible), `(from Sin, Forgiveness of)` for a followed `See`, and
  `see` links to other topics (`SIN, FORGIVENESS OF` opens `Sin`). ▶ shows the first 8 Passages'
  text inline (one line each), per subtopic of each entry (a topical index can have several
  entries on a topic). The first 12 subtopics of each entry, then Show all.
- Key Passages: the Passage link, `cited N×`, and its first line from the ★ Bible.
- A Passage click moves the Topic Guide's Link set (a Bible in it goes there); unlinked, the most
  recent Bible Tab goes there (or the ★ Bible opens); Ctrl/Cmd opens the ★ Bible in a new Tab.

## Evidence

- Engine `topic_guide_tests`: Forgiveness (Nave's subtopics and items, `See`, the followed
  `See SIN, FORGIVENESS OF`, Torrey's subtopics, Easton's `Forgiveness of sin`, Key Passages,
  Search), the chooser with synonyms, the header box and command, Link sets, Favorites, the
  Command box, empty states, and line parsing.
- Formats `sword::tei_topical_index_keeps_each_subtopic_item_a_paragraph` (fixture modules
  `ObsNave` TEI and `ObsTorrey` ThML, abridged from the public-domain works).
- UI `shell::tests::guide::topic_guide_on_forgiveness_shows_naves_subtopics_and_a_passage_moves_the_link_set`.
- Real library (manual): import CrossWire's `Nave` and `Torrey` with `obs-cli import`, copy the
  Starter's `easton.obr`, then `OBS_TOPIC_LIBRARY=<dir> cargo test -p obs-engine
  real_library_forgiveness -- --ignored --nocapture`.
