# Markdown books (#344)

A **Book** is the seventh Resource type ([ADR 0023](../adr/0023-a-book-is-a-seventh-resource-type-read-by-divisions-and-printed-pages.md)):
prose read from its start, by its **Divisions** and **printed pages**, with no Key scheme.
Books come from **Markdown**: the Christian Open Library's (COL) download (#342, #347), an
Obsidian vault, or any book kept the same way.

## Import

**Library → Import… → Markdown book…** picks a `.md` file. The engine reads the book that file
belongs to: its main file and the Part files beside it. Choosing a folder (`import.start` with a
folder path) works when the folder holds one book; a folder of several books is refused with
"That folder holds several books. Choose one book's Markdown file." (importing a whole folder,
one Resource per book, is still to come in #344).

`ImportSource::detect` treats a `.md`/`.markdown` file, or a folder with Markdown files and no
`.usfm`/`.sfm` files, as `ImportSource::Markdown`. The new Resource's id is made from the book's
stem, the file name without its extension or `— Part N`.

## Reading the files (`obs_formats::markdown_book`)

| Source | Result |
|---|---|
| Front matter `title`, `author`, `date`/`year`, `license`, `source`, `language` | manifest title, publisher (author), edition (year), licence, source URL, language (default `en`) |
| A `#` heading naming the book among a file's first three blocks, and a `*by …*` line under it | the title; not text |
| The shallowest heading level used more than once in the book | Divisions: one text unit each (`div.1`, `div.2` …, kind `Division`, titled by the heading); text before the first is "Front matter"; with no such level, one unit per file ("Part 1" …) |
| Deeper headings | `heading` spans |
| Paragraphs, list items | `para` prose / list; a paragraph with hard breaks (`\` or two spaces at a line's end) is verse: one `poetry1` line each. A plain line break is a space, as CommonMark reads it, so prose wrapped in its file, or OCR text keeping the scan's lines, stays one paragraph |
| `*…*`, `**…**` | `italic`, `bold` |
| `<!-- p. 23 -->` | zero-width `page` span, label "23" (`{iii}`, `page 4`, `pg323` read as COL's indexer reads them); a marker before a block opens it, so a page that starts with a Division's heading opens that Division; a marker on its own line between two lines of a paragraph stays inside the paragraph (CommonMark would otherwise end the paragraph there) |
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

Still to come (#344): importing a folder of books in one go, the Contents pane's Division list,
Go to page…, the Passage Guide's "Cited in" section, and long-s/u-v/i-j search normalisation for
EEBO texts.
