# A Book is a seventh Resource type, read by its Divisions and printed pages

Most historic Christian writing is neither Scripture nor keyed to anything: sermons, treatises,
confessions, catechisms, hymnals, journals and church records. The Christian Open Library (COL)
offers more than 7,600 such works as Markdown files (#342), and users keep other books the same
way (Obsidian vaults, Project Gutenberg and Standard Ebooks texts). None of the six Resource
types fits them. A Commentary is keyed by Passage, a Lexicon or Dictionary by Lemma or headword,
and a book is keyed by neither: it is read from its start, or from a chapter, or from a printed
page someone cited. We decided (#344):

1. **Book is the seventh Resource type** (`ResourceType::Book`): a Resource of prose organised
   by its own **Divisions** (its chapters and sections, from its headings) and, where the source
   gives them, its **printed pages**. It declares no Key scheme. It is never a lookup target
   (Word Study, Factbook, Passage Guide Commentaries, Link sets by Key), and it has a Preferred
   order like every type, which only orders it in lists.
2. **One text unit per top Division.** The text is split at the shallowest heading level used
   more than once under the title (a `#` title that opens each file is the title, not a
   Division); text before the first such heading is the Division `Front matter`. A book with no
   such headings is one unit per source file. Unit ids are `div.<n>` in document order; deeper
   headings are `heading` spans inside the unit. The Contents pane lists Divisions and the
   headings inside them.
3. **Printed pages are a new zero-width span kind, `page { label }`.** Its label is the page as
   printed ("23", "iv", "14a"). It is what a quotation cites and what Go to page… opens. This is
   `.obr` format 5. Older files never contain the span, so the reader still opens formats 3 to 5
   (ADR 0010); only the content hash of every Resource changes once, as with format 4.
4. **A Book Tab has no Key location.** Its place is a text position (unit and byte), kept as the
   Tab's scroll position in Layouts and the Last session. A jump inside a Book (Contents, Go to
   page…, a search hit) moves the view as a Library search hit with no Key does today; making
   those jumps Back/Forward steps (a history of text positions) is left to a later change.
5. **Markdown books are imported by our own reader** of the conventions COL and similar
   libraries use: YAML front matter of plain `key: value` scalars (`title`, `author`, `date`,
   `license`, `source`; no YAML anchors, aliases, merges or nesting are read), CommonMark text
   (pulldown-cmark, with footnotes), page markers as HTML comments (`<!-- p. 23 -->`), and long
   works split into files named `<Title> — Part N.md` or `<Title>, Part N.md` beside the main
   file. Parts are joined into one Resource in their numeric order. Markup outside the span
   vocabulary (images, tables, raw HTML) is dropped and counted in the Import report, as for
   every other importer.
6. **Text is kept as printed.** No spelling is modernised; `ſ`, `vv` and `&` stay. Bible
   references in the text, in the forms older books write them (`Rom. 8. 28`, `Rom. viii. 28`),
   become `scripture_ref` spans in the Standard versification.
7. **The licence is the file's own.** The `license:` value maps to an SPDX id only through a
   tested allowlist of whole values (`public domain`, `cc0`, `cc-by-4.0`, `cc-by-sa-4.0`, …);
   anything else is `LicenseRef-unknown`, so the Resource is Unclear and never shown as free, and
   the value itself is kept in the About view. `source:` is the source URL.

## Considered Options

- **A Dictionary keyed by Division title** (the ADR 0009 route): no new type, but headword
  lookups (Word Study, the Factbook, a word dropped on a Dictionary Tab) would find "Chapter I"
  and "Sermon XII", and a Dictionary's Contents is A–Z, not the book's order.
- **A Commentary with no Passage Keys:** the Passage Guide, Parallel resources and the Link set
  all assume a Commentary answers a Passage; every one would need an exception.
- **A new Key scheme, `Section`, for Divisions:** Link sets would offer to follow it and the
  Back/Forward history would gain a second kind of Key for one type. Divisions are navigation,
  not something two Resources share, so a text position is enough.
- **Page labels inline as text (`[p. 23]`, as COL's own search shows them):** no format change,
  but the label would be read, searched and quoted as words of the book.
- **A Book per Division file, or one unit per file:** COL's Part files are cut by size, not by
  the book's structure, so a chapter could start in one Part and end in the next.
- **A YAML crate for front matter:** COL's own indexer refuses aliases and merge keys because
  they can expand a few bytes into gigabytes, and keeps only scalars; reading `key: value` lines
  ourselves gives the same result with no new parser to harden.

## Consequences

- Every place that lists or matches Resource types gains Book: the `.obr` manifest, the
  Library Tab and launcher, Home's Your library, Preferred order, Collections rules, Search
  scope, Parallel resources (other Books), and the empty states. An app built before this one
  refuses a Book `.obr` (format 5) with its usual "newer format" message.
- Format 5 changes every content hash once: the Starter build gives each Resource a new
  revision, and Annotations re-anchor to unchanged text (ADR 0006), as at format 4.
- Unit ids by order are stable while a book's top Divisions keep their order; a revision that
  inserts a chapter shifts the ids after it, and Annotations there re-anchor by quote and
  context (ADR 0006).
- Get Resources and the COL download (#347) produce Books through the same importer; Bible
  references in Books feed a Passage Guide section later ("Cited in"), and are found by Library
  search now.
