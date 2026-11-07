# Parallel passages ride on a Cross-reference set, from the BSB's own section references

The Passage Guide's Parallel passages section (#212) lists the other accounts of a Passage: the
Gospels' tellings of one event, Chronicles beside Samuel and Kings. It needs a table of sections
and the Passages parallel to each, offline. The spike ([research](../research/parallel-passages.md))
found one open, machine-readable source covering both the Gospels and the histories: the
Berean Standard Bible's own USFM, whose `\r` line under each section heading lists the parallel
passages (public domain, and already pinned for the `bsb` Starter Bible). We decided three
things that are hard to undo once Starter libraries carry them:

1. **Parallel passages are a dataset of a Cross-reference set, not a seventh Resource type.** A
   Cross-reference set may carry `Resource::parallel_sections`: each a `ParallelSection` (the
   section's Passage, its heading, and the parallel Passages in the source's order), all in the
   Standard versification. In the `.obr` they are the optional `parallel_sections` and
   `parallel_passages` tables (older files read none, like `events`); they count in the content
   hash only when present, so no other Resource's hash changes. The BSB's ship as
   `bsb-parallels`, a set with Parallel passages and no Cross-references.
2. **The source's sections are stored as written, and the section reads them both ways at
   query time.** The BSB's sections differ in size from book to book (Mark 1:1-11 lists Matthew
   3:1-17; Matthew 3:13-17 lists Mark 1:9-11). Storing them verbatim keeps the data the
   publisher's and reviewable; `obs_engine::parallel_passages` keeps the narrowest evidence, so
   Mark 1:9-11 lists Matthew 3:13-17, not all of Matthew 3.
3. **A click compares the parallel without moving the Guide.** `compare.parallel` puts Text
   Comparison on the parallel Passage (its range chip) in a Tab outside the Guide's Link set,
   reusing the most recent such Text Comparison or opening one in a Panel beside the Guide. Cross-reference clicks move the Link set; a
   parallel is read beside the Passage, not instead of it.

## Considered Options

- **A seventh Resource type ("Harmony", "Parallels"):** names it, but every place that lists
  Resource types gains a case and an older app could not open the file; ADR 0009 and 0011
  already keep tool-read datasets within the six types. A Cross-reference set is the Resource
  that links Passages to Passages.
- **Parallels as plain Cross-references (from → to, no votes):** no new table, but the heading
  that pairs them is lost, the Cross-references section would list them twice, and nothing
  could tell a parallel from a related Verse.
- **Groups of Passages normalised at build time (one row per event):** reads simply, but the
  BSB's overlapping sections do not partition into events without heuristics that rewrite the
  publisher's data; the query-time rule is small and tested.
- **Wikipedia's Gospel harmony table (CC BY-SA 4.0) or a transcribed public-domain harmony
  (Robertson 1922, Crockett 1897):** Gospels only, or no clean source (CCEL refused, scans only).
  See the research note.
- **Open Text Comparison in the Guide's Link set:** the Guide and its Bible would jump to the
  parallel and lose the Passage being studied.

## Consequences

- The section's coverage is the BSB's: Gospels, Samuel/Kings ↔ Chronicles, Psalms ↔ Samuel, the
  Law's repeated laws, Isaiah 36-39 ↔ 2 Kings and the Old Testament passages the New quotes. A
  few BSB pairings are thematic (John 3 beside Genesis 22); each row shows the BSB heading that
  pairs them.
- An imported Cross-reference set with Parallel passages shows in the section as it is; the
  empty state offers only the sets in `parallel_passages::PARALLEL_SETS`, and a set listed there
  never fills the Cross-references section's needs.
- The `usfm-parallel-passages` converter reads any USFM Bible's `\r` lines in the Standard
  versification; a Bible numbered otherwise would need its Verse map applied first.
