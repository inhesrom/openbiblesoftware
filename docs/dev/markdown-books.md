# Markdown books (#344)

A **Book** is the seventh Resource type ([ADR 0023](../adr/0023-a-book-is-a-seventh-resource-type-read-by-divisions-and-printed-pages.md)):
prose read from its start, by its **Divisions** and **printed pages**, with no Key scheme.
Books come from **Markdown**: the Christian Open Library's (COL) download (#342, #347), an
Obsidian vault, or any book kept the same way.

## Import

**Library → Import… → Markdown book…** picks a `.md` file. The engine reads the book that file
belongs to: its main file and the Part files beside it (`ImportSource::Markdown`, one Import, one
Resource, the usual report). The new Resource's id is made from the book's stem, the file name
without its extension or `— Part N`.

**Markdown folder…** (or `import.start` with a folder that holds Markdown files, in it or its
subfolders, and no `.usfm`/`.sfm` of its own) is a **book batch** (`obs_engine::books`):

- `FolderStream` lists every Markdown file under the folder (hidden folders such as `.git` and
  `.obsidian`, and `_trash`, skipped; symbolic links not followed; at most 100,000 files) and
  groups them into books (`group_book_files`, per folder).
- One job (`BatchJob`) imports one book per step, so Cancel stops between books. Each book is
  written as its own Book Resource with `import::write_resource` (the same writer and Search
  indexer a single Import uses) and sent back at once with `JobContext::emit` (a new
  `JobEventKind::Partial`), so it joins the Library while the rest are still being read, with
  its own Import record ("Owen.md in /path/to/books").
- The batch has one Import report Tab (`TabKind::ImportReport("books:N")`; `:` never appears in a
  Resource id). Its record's `books` (`BookBatchSummary`) lists each book brought in (each opens
  the Book) and each that could not be, with the plain reason and the error on hover. A book that
  fails (no text, not UTF-8, too large) never stops the others; the finish message counts both
  ("Imported 2 books; 2 could not be imported").
- A book whose `LoadedBook::resource_id` names a Resource in the Library is written as its next
  Resource revision (the update path #347 uses); a folder import always makes new Resources.

## Reading the files (`obs_formats::markdown_book`)

| Source | Result |
|---|---|
| Front matter `title`, `author`, `date`/`year`, `license`, `source`, `language` | manifest title, publisher (author), edition (year), licence, source URL, language (default `en`) |
| A `#` heading naming the book among a file's first three blocks, and a `*by …*` line under it | the title; not text |
| The shallowest heading level used more than once in the book | Divisions: one text unit each (`div.1`, `div.2` …, kind `Division`, titled by the heading); text before the first is "Front matter"; with no such level, one unit per file ("Part 1" …) |
| Deeper headings | `heading` spans |
| Paragraphs, list items | `para` prose / list; a paragraph with hard breaks (`\` or two spaces at a line's end) is verse: one `poetry1` line each. A plain line break is a space, as CommonMark reads it, so prose wrapped in its file, or OCR text keeping the scan's lines, stays one paragraph |
| `*…*`, `**…**` | `italic`, `bold` |
| `<!-- p. 23 -->`, or `[p. 23]` on a line of its own (COL's church documents) | zero-width `page` span, label "23" (`{iii}`, `page 4`, `pg323` read as COL's indexer reads them); a marker before a block opens it, so a page that starts with a Division's heading opens that Division; a marker on its own line between two lines of a paragraph stays inside the paragraph (CommonMark would otherwise end the paragraph there) |
| `[^1]` and `[^1]: …` | a `note.N` unit and a zero-width `note_ref` footnote span |
| `Rom 8:28`, `Rom. 8. 13`, `Rom. viii. 28`, `Psal. 23. 1` | `scripture_ref` spans, Standard versification (`book_references`) |
| Links | the link text; the target is dropped |
| Images, tables, code blocks, raw HTML | dropped, counted in the Import report's dropped markup |

**Parts.** COL splits a long work into `<Title> — Part N.md` (or `<Title>, Part N.md`) beside
`<Title>.md`, whose body is only a list of links to the Parts. `group_book_files` groups a
folder's names into books (Parts in numeric order: 1, 2, 10). With Parts, the main file gives
only the front matter and the Parts give the text, read as one stream, so a chapter that runs
from one Part into the next is one Division.

**Front matter** is read by hand, not with a YAML parser: plain `key: value` lines, quoted or
not, each value at most 2,000 characters; lists, nesting, anchors (`&`), aliases (`*`), block
scalars and tags are skipped, and front matter over 16 KB or never closed is text. COL's own
indexer refuses YAML aliases and merges for the same reason (a few bytes can expand without
bound).

**Licence.** `book_license_spdx` maps the whole `license:` value, case, spaces, `-` and `_`
aside, to an SPDX id from a short allowlist (public domain, CC0, CC BY 4.0, CC BY-SA 4.0).
Anything else is `LicenseRef-unknown`: the Resource's Licence class is Unclear, the Import report
says the licence was not recognised, and the value is kept in the Attribution ("Licence as
given: …"). As with SWORD confs, free text is never read for its meaning.

**Limits.** A file over 64 MB is refused (`ImportError::TooLarge`); a book with no text is
refused (`ImportError::NoText`); a non-UTF-8 file is refused with its name. The parser is
pulldown-cmark (CommonMark, footnotes and tables on), as for Note text; the hostile-input test
feeds it deep nesting, unbalanced brackets and emphasis, thousands of footnotes, unclosed
comments and oversized front matter.

## `.obr` format 5

The `page` span kind (id 17, its label as a string) is new, so `FORMAT_VERSION` is 5. Formats 3
and 4 never hold it and still open (ADR 0010); every Resource's content hash changes once.
`docs/dev/obr-span-storage.md` lists the id.

## Reading

A Book opens in a Resource Tab like a Commentary or Dictionary; the reading pane draws a printed
page as `[p. 23]`, small and raised like a verse number (`InlineObject::PageNumber`), at the
place the page begins. Books are a Library search type (`LIBRARY_TYPES`, after Dictionaries), a
Collection rule value (`type:book`), a Library launcher section and a Library Tab group.

- **No Link set.** A Book follows no Key (`obs_engine::tab_key_schemes` gives it none), so its
  Tab has no Link-set chip (`Tab::has_link_chip`), never joins its opener's Link set, and
  `tab.link` refuses it. A Bible moving never moves a Book.
- **Go to page.** Its header box is `HeaderBox::Page` ("Go to page"): it shows the printed page
  at the top of the view (`ResourceNavigation::page_at`, from the Tab's scroll position) and
  `tab.go` with "23", "p. 23" or "page 23" jumps there (`ResourceNavigation::page`, the first page
  with that label, case aside). The reading view builds the page list from the `page` spans
  (`ReadingDoc`'s navigation, `with_pages`).
- **Contents.** One line per Division in the Book's order (`Contents::book`,
  `ContentsTarget::Unit`), with a "Filter chapters" box; clicking a line goes to the Division's
  start.
- **Cited in.** The Passage Guide's last section (`GuideSection::CitedIn`, `obs_engine::cited_in`)
  lists the Books that cite the Passage, earliest first (by the year in their edition, then
  title): author, year, how many places, the Division and paragraph of the first, the reference
  highlighted. Each Book's `scripture_ref` spans are read once per revision with
  `ObrReader::scripture_references` (one scan of the span table, note units left out) into a
  compact `BookCitations`; only a Book that cites the Passage is opened again, for its excerpt.
  Clicking a row opens the Book at that reference (Ctrl/Cmd: a new Tab).

Not done: long-s/u-v/i-j search normalisation for EEBO texts (a Search index change of its own),
and Back/Forward steps between jumps inside a Book (ADR 0023 point 4).
