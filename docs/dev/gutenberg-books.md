# Project Gutenberg books (#345)

Project Gutenberg (PG) has transcribed some 75,000 books not protected by U.S. copyright,
the 1800s' sermons, devotional works and theology among them. Its texts are the book's text
plus PG's licence and trademark; PG's licence says a text stripped of both is unrestricted
(evidence: `crates/engine/data/open-evidence/gutenberg.md`). The Open catalogue offers PG
books as a source collection (ADR 0024): `crates/engine/data/collections/pg-books.tsv`.

## The collection

```text
# number	path	sha256	bytes	language	authors	died	title
30449	3/0/4/4/30449/30449-h/30449-h.htm	<64 hex>	230931	en	Moody, Dwight Lyman, 1837-1899	1899	The Way to God and How to Find It
```

`cargo xtask pg-books` writes it (`xtask/src/pg_books.rs`):

- `--add numbers.txt` adds eBook numbers (one per line, `#` comments);
- `--refresh` downloads every row's file again and re-pins it;
- with neither, every row is checked again from the cache (`--cache`, default
  `target/pg-books`).

It reads PG's catalogue (`pg_catalog.csv`, downloaded from the mirror unless `--catalog` names
a copy) and, for each book, the folder listing of its posted HTML on PG's mirror
(`https://gutenberg.pglaf.org/<folder>/<n>-h/`). It downloads one file at a time with a pause,
as PG asks of programs, and never from www.gutenberg.org. A book is added only when:

- PG's catalogue lists it as a Text and gives a death year for every person it names (author,
  translator, editor, …), none later than `PG_LAST_DEATH_YEAR` (1955): 70 years on, the book is
  out of copyright in life-plus-70 countries too, not only in the United States, where PG has
  checked it. "Anonymous", "Various" or an open-ended date leaves it out;
- PG does not mark it "COPYRIGHTED Project Gutenberg eBook";
- its text does not name ccel.org or the Christian Classics Ethereal Library: a transcription
  made for CCEL is never used, whoever hosts it (#20; PG's *Summa Theologica* is one);
- it imports as a Book with at least one Division.

A book PG posts no HTML of is read from its plain text (`<n>-0.txt`, `<n>.txt`, `<n>-8.txt`)
by `obs_formats::import_pg_text` (`pg_text.rs`): chapters after three or more blank lines (or,
with fewer than three, paragraphs starting "CHAPTER", "SERMON", "QUESTION", …), subtitles in
capitals, indented lines as poetry, `_italics_`, and `{23}` page marks.

The engine makes each row an entry (`PgBook::source`): id `pg-<n>`, importer `pg-html` (or
`pg-text` for a text file), the
mirror URL pinned by sha256, the title cut at its first colon, the first author who is not a
translator or other contributor as a name (`display_author`), "Public domain"
(`LicenseRef-PublicDomain`, free-to-share), PG's archived licence page as `evidence_url`.

## Project Gutenberg's name

PG's licence forbids using its trademark on a text that has dropped the licence, but allows
references to PG, with links, in acknowledgements, reference sections and colophons. So:

- the Book's text holds nothing of PG: the importer reads only between PG's
  "*** START OF THE PROJECT GUTENBERG EBOOK" and "*** END OF …" lines and drops every block
  that names Gutenberg;
- PG is credited, with a link to the eBook's page, in the Book's Attribution (its About view)
  and edition, and is the row's source ("from Project Gutenberg") with the link.

## The importer

`obs_formats::import_pg_html` (`crates/formats/src/html_book.rs`) reads PG's hand-made HTML with
a tolerant tokenizer (HTML 4 character references, `script` and `style` skipped, unclosed
tags closed by the next block); a file that is not UTF-8 is read as Windows-1252
(`decode_html_bytes`). The module's table maps HTML to the Book. In short:

- Divisions at the shallowest heading level used three times or more (else twice, else once);
  a Division holding only headings (a title page's, a part's) opens the next one;
- printed pages from `class="pagenum"` (and `pgmark`, `pageno`, …), or, in a book with none,
  from anchors `id="Page_23"`; a transcriber's bracket spans (`class="ns"`) and links back to
  the contents are not text;
- footnotes: a reference is an `a` to an id starting `Footnote`, `FN` or `note`, or with
  `class="fnanchor"`; the note is the block (or `class="footnote"` container) that the
  referenced anchor opens; a reference whose note never comes is dropped;
- poems (`class="poem"`, `"stanza"`) line by line, `pre` line by line, lists, italic, bold,
  a heading printed as an image read from its alt text, Bible references as links.

## Tests

- `crates/formats/src/html_book_tests.rs`: a sample with PG's shapes (both footnote styles, page
  numbers, a poem, PG's header and footer), tolerant tokenizing, Windows-1252, hostile input,
  and an ignored check of a folder of real files
  (`OBS_PG_DIR=… cargo test -p obs-formats --release --lib every_book_in_a_folder -- --ignored --nocapture`).
- `open_catalogue::tests::pg_collection_rows_are_sound` and `a_pg_row_makes_an_entry_and_reads_back`.
- `xtask`: death years, CSV and titles.
- Live, by hand: `live_every_open_catalogue_entry` downloads every 100th collection row.
