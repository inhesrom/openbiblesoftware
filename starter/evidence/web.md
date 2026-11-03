# Evidence: World English Bible (`web`)

- **Edition shipped:** eBible.org `engwebp`, "the updated World English Bible with the 66-book protocanon only" (2020 stable text edition), USFM source files dated 8 Oct 2026.
- **Pinned file:** <https://web.archive.org/web/20261011232837id_/https://ebible.org/Scriptures/engwebp_usfm.zip> (Internet Archive capture of <https://eBible.org/Scriptures/engwebp_usfm.zip>, 2026-10-11). eBible regenerates its zips, so the live file's hash drifts; the archived capture is the pinned edition.
- **sha256:** `674fb7eeffa9d64b90c4565a3678ffed61a1ddba47dff9b6a618528e2aaa8c67`
- **Archived licence notice:** [`web-copr.htm`](web-copr.htm), the `copr.htm` inside the pinned zip.

## Licence

The notice in the pinned zip says:

> The World English Bible is in the Public Domain. That means that it is not copyrighted. However, "World English Bible" is a Trademark of eBible.org. … All we ask is that if you CHANGE the actual text of the World English Bible in any way, you not call the result the World English Bible any more.

SPDX: `LicenseRef-PublicDomain`. The Starter build keeps the verse text verbatim (it omits only eBible's front matter and glossary book and the book introductions), so the name is kept.
