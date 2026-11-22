# Evidence: Project Gutenberg books (`pg-*`)

This file is the licence evidence for every row of the Project Gutenberg (PG) source collection,
`data/collections/pg-books.tsv` (ADR 0024, #345).

- **Download:** each eBook's posted HTML (`<n>-h/<n>-h.htm`) from PG's own mirror,
  <https://gutenberg.pglaf.org/>, run by the Project Gutenberg Literary Archive Foundation. PG's
  terms ask that programs download from a mirror, not from www.gutenberg.org. PG's list of
  mirrors names it "Project Gutenberg … High-speed mirror", captured at
  <https://web.archive.org/web/20261108165357/https://www.gutenberg.org/MIRRORS.ALL>.
- **Pinned:** each row's sha256; any other bytes are refused. PG regenerates its EPUB and
  "-images" files; the posted HTML changes only when the book itself is corrected.
- **Archived licence page:** <https://web.archive.org/web/20261116094507/https://www.gutenberg.org/policy/license.html>
  (Internet Archive capture of <https://www.gutenberg.org/policy/license.html>).

## Licence

A PG eBook of a book not protected by U.S. copyright is the book's text plus PG's trademark and
licence. PG's licence page says:

> If you strip the Project Gutenberg license and all references to Project Gutenberg from the
> text, you are left with a text unrestricted by U.S. intellectual property law. You can do
> anything you want with that text in the United States and most of the rest of the world.

> You may still distribute the ebooks if you strip the Project Gutenberg license and all
> references to Project Gutenberg.

> Examples that do not constitute use of the Project Gutenberg Trademark: references to Project
> Gutenberg (with links, please!) in acknowledgements sections, reference sections or colophons.

The importer (`obs_formats::import_pg_html`) reads only the text between PG's
"*** START OF THE PROJECT GUTENBERG EBOOK" and "*** END OF …" lines and drops every block that
names Project Gutenberg, so the Book holds no PG licence or trademark. PG is credited with a link
in the Book's Attribution (its About view, the colophon) and named as the source of the download.

SPDX: `LicenseRef-PublicDomain`. Licence class: free-to-share.

## Which books

PG checks that a book is not protected by U.S. copyright, and says nothing of other countries.
A row is added only when every person PG's catalogue names for the book (author, translator,
editor, writer of an introduction, …) has a known death year no later than **1955**
(`PG_LAST_DEATH_YEAR`): 70 years on, the book is out of copyright in countries whose term is the
author's life plus 70 years as well. A book PG marks as copyrighted ("COPYRIGHTED Project
Gutenberg eBook") is never added, nor one whose text says it came from the Christian Classics
Ethereal Library (ccel.org), whose transcriptions are never used (#20). `cargo xtask pg-books` applies both rules, and checks that the
book imports.
