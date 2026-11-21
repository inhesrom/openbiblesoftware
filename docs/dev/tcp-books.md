# TCP books (#345)

The Text Creation Partnership (TCP) keyed and encoded some 60,000 early printed books in TEI
XML: EEBO-TCP (English, 1473–1700), Evans-TCP (America, 1639–1800) and ECCO-TCP (the 1700s).
Every text is published as its own GitHub repository under
<https://github.com/textcreationpartnership>, and every one is released under CC0 1.0: each
file says so in its `<availability>`, and TCP's FAQ dates the releases (Evans 2014, EEBO
Phase I 2015, Phase II 2020). They are the first source of historic Books in the Open
catalogue: the originals, from their publisher, as #345 plans.

## Hand-written entries

Ten texts, listed as Open catalogue entries with `type = "book"` and `importer = "tcp-tei"`
(`crates/engine/data/open-catalogue.toml`), one shared evidence file
(`data/open-evidence/tcp.md`) quoting both licence statements and the archived FAQ:

| Entry | TCP id |
|---|---|
| Owen, *Of the Mortification of Sin in Believers* (1668) | A53715 |
| Westminster Assembly, *Confession of Faith* (1646) | A96226 |
| Bunyan, *The Pilgrim's Progress* (1678) | A30170 |
| Baxter, *The Reformed Pastor* (1656) | A26932 |
| Sibbes, *The Bruised Reed* (1630) | A12171 |
| Brooks, *Precious Remedies against Satan's Devices* (1658) | A77614 |
| Alleine, *An Alarm to Unconverted Sinners* (1672) | A26682 |
| Flavel, *A Saint Indeed* (1668) | A39682 |
| Edwards, *Sinners in the Hands of an Angry God* (1741) | N03833 |
| Bunyan, *Grace Abounding to the Chief of Sinners* (1666) | A30143 |

Each `url` is the raw file at a fixed commit
(`https://raw.githubusercontent.com/textcreationpartnership/<id>/<commit>/<id>.xml`), so it
never changes, and `sha256` pins it (ADR 0008). The title, abbreviation and edition are the
entry's: TCP's own titles are the title pages, a paragraph long. Get Resources' **Books** chip
(`ResourceFilter::Type(ResourceType::Book)`, `books` in commands) shows them in the Open
catalogue section.

The hand-written entries have curated titles (TCP's own are title pages, a paragraph long),
author and date, and are the Open catalogue section of Get Resources' Books filter.

## The TCP collection

Every other TCP text the app offers is a row of `crates/engine/data/collections/tcp-books.tsv`,
a source collection ([ADR 0024](../adr/0024-a-source-collection-is-a-generated-pinned-list-checked-when-a-text-is-added.md)):

```text
# id	commit	sha256	bytes	language	date	author	title	statement
A00001	<40 hex>	<64 hex>	123456	en	1641.	Owen, John, 1616-1683.	A short title	cc0-phase1
```

`cargo xtask tcp-books` writes it (`xtask/src/tcp_books.rs`):

- `--add ids.txt` adds TCP ids (one per line, `#` comments), each pinned at its repository's
  current commit (`git ls-remote`, then the raw file with `curl`, cached under `--cache`,
  default `target/tcp-books`, as `<id>-<commit>.xml`);
- `--refresh` re-pins every row at its repository's current commit;
- with neither, it checks every row again.

A text is added only when its `<availability>`, read by `obs_formats::read_tei_header`, is one of
`TCP_CC0_STATEMENTS` whitespace aside (`tcp_statement`), its header names the same id, and
it imports as a Book with at least one Division; the rest are listed as left out. Rows are
sorted by id, and a text a hand-written entry lists is skipped.

The engine reads the file at build time (`open_catalogue::tcp_books`) and makes each row an
entry (`TcpBook::source`): id `tcp-<id>` lowercased, its title, the author as a name
(`display_author`: "Owen, John, 1616-1683." → "John Owen"), the date, the language, the TCP
id as abbreviation, "Text Creation Partnership" as publisher, CC0 1.0, `open-evidence/tcp.md`
as evidence and TCP's archived FAQ as `evidence_url`. Its rows are marked `collection`: Get
Resources lists them in All Books, not in the Open catalogue section, and its search matches
the author as well as the title.

To find texts by subject, TCP's catalogue (`TCP.csv` in
<https://github.com/textcreationpartnership/Texts>) has Library of Congress subject headings
(`Terms`); its `Status` column predates the Phase II release and can still say "Restricted".

## The importer

`obs_formats::import_tcp_tei` (`crates/formats/src/tcp_tei.rs`) reads one TCP text with
`roxmltree`: no DTD (an entity is refused, not expanded), at most 20 million nodes, at most
128 MB, nesting past 64 read as plain text. The module's table maps TEI to the Book:

- **Divisions** are the `<div>`s of `<front>`, `<body>` and `<back>`. A `<div>` holding two or
  more `<div>`s (a part of chapters, a sermon of uses) is split into them; a short part head
  ("Part I.") prefixes their titles. A Division is titled by its `<head>`, else by its `type`
  and `n` ("Title page", "Chapter 2"), cut at a word at 120 characters.
- **Printed pages:** `<pb n="23">` is a zero-width `page` span "23". A `<pb>` without `n` is a
  leaf TCP did not number (a title page, a blank): no page. A page turned between blocks
  opens the next one.
- **Notes:** each `<note>` (TCP's margin notes) is a note unit, referenced by a footnote.
- **Words broken at a line's end** (`<g ref="char:EOLhyphen"/>`) are joined.
- **Combining marks** TCP keys in an element of their own (`whe<g ref="char:cmbAbbrStroke">̄</g>`)
  are composed with their letter: `TextBuf::normalize_nfc` puts the unit in NFC and moves its
  spans.
- Spelling stays the printer's (`ſ`, `vv`, `u`/`v`); `<gap>` shows as its description or
  "〈…〉"; figures and tables are dropped and counted in the Import report.
- **Bible references**, in the text and the notes, are found as in a Markdown book
  (`commentary::book_references`), Roman chapter numbers included ("Rom. viii. 13").

The importer sets no licence: `OpenSource::import` writes the entry's (`CC0-1.0`), with its
Attribution and changes.

## Tests

- `crates/formats/src/tcp_tei_tests.rs`: a sample with TCP's structure, Edwards's sermon and
  the Westminster Confession (`crates/formats/tests/assets/tcp/`, the published files, CC0),
  hostile input, and an ignored check of a folder of texts
  (`OBS_TCP_DIR=… cargo test -p obs-formats --release --lib every_text_in_a_folder -- --ignored --nocapture`).
- `open_catalogue_tests::a_tcp_book_downloads_by_its_pin_and_imports_as_a_book`: the Books
  filter, the download checked against the real pin, the Book in the Library, a changed file
  refused.
- `open_catalogue::tests::tcp_collection_rows_are_sound` and `tcp_statements_match_the_files`:
  every row of the collection, and the statements against real files.
- Live, by hand: `cargo test -p openbible --release -- --ignored --nocapture
  live_every_open_catalogue_entry` downloads every hand-written entry and every 100th
  collection row (all of them with `OBS_LIVE_ALL_COLLECTIONS=1`).
