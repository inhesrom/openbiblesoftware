# Commentary sources for the Starter library (issue #39)

**Question:** Which Matthew Henry (MHC) and Jamieson-Fausset-Brown (JFB) editions pass the Starter provenance gate (#20 decision 3: archived licence evidence for the exact edition shipped, no CCEL-hosted files)?
**Date:** 2026-11-03. All sources fetched and archived that day.
**Inputs:** #20 decision 3 and its handed-on risks; [open content inventory](open-content-inventory.md) §4.
**Output:** a ship / fallback / drop decision for each commentary, evidence files under `starter/evidence/`, and proposed lock entries for #44.

## TL;DR

| Commentary | Decision | Source | Pinned sha256 | Coverage |
|---|---|---|---|---|
| **Matthew Henry, Complete** (`mhc`) | **Ship** | English Wikisource transcription, 134 pages, as one MediaWiki XML export captured by the Internet Archive | `149a9a9fe7bfef10ab4d3ae99662051fe90a19243e53735ce85f323db30e0934` | 66/66 books, 1,189/1,189 chapters; only Luke 18:9-43 missing (99.86% of KJV verses inside a section) |
| **Jamieson-Fausset-Brown** (`jfb`) | **Ship** | Woodside / OnLine Bible "expanded electronic edition" (ed. Ernie Stefanik), `JFB-UPD3.zip` from thirdmill.org, captured by the Internet Archive | `b242392883f5f26a8e0a8cfc4995e9cdb73fa165d9dd754b057ce039c8f258ec` | 66/66 books, 1,189/1,189 chapters, comment text for all 31,102 KJV verses (31,100 have their own anchor) |

- **Wikisource MHC is complete.** The "volumes 1–3 and 6" worry in #20 is out of date: the table of contents now lists all six volumes, and every Bible book has full text. The fallback (Matthew Henry's *Concise*) is not needed and was not evaluated further.
- **Wikisource got its MHC text from CCEL.** The work's talk page gives "Source: Christian Classics Ethereal Library". We take no CCEL-hosted file, and Wikisource tags the work `{{PD-old}}`, so it passes the gate as written. But this is the main risk of this spike (see [Risks](#risks)).
- **JFB has a clean non-CCEL host.** Third Millennium Ministries (thirdmill.org) serves the editor's own 75-file archive. The edition says on every page that it "is in the public domain", and its notice adds "may be freely used and distributed". The Internet Archive's 2012 capture matches the live file byte for byte.
- Neither source is added to `starter/sources.toml` here; that is #44's job, together with the converters. The [proposed lock entries](#proposed-lock-entries) are checked against the provenance gate by the test `the_proposed_commentary_lock_entries_pass_the_provenance_gate`.

## Matthew Henry's Commentary on the Whole Bible

### Source chosen

- **Work:** Matthew Henry, *An Exposition of the Old and New Testament* (1706–1721). Henry died in 1714; Romans to Revelation were completed from his notes by other nonconformist ministers and published in 1721.
- **Wikisource title:** [Matthew Henry's Commentary on the Whole Bible](https://en.wikisource.org/wiki/Matthew_Henry%27s_Commentary_on_the_Whole_Bible). There are 134 pages under that prefix: the main page, 6 volume pages, 6 volume prefaces, the book pages, and subpages for Genesis (22), 1 Corinthians (18), Philippians (6), Luke 18 and the split John and Matthew pages. `Volume 4/Amon` is a redirect to `Volume 4/Amos`.
- **Pinned file:** <https://web.archive.org/web/20261103225156id_/https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew+Henry%27s+Commentary+on+the+Whole+Bible&gaplimit=max&export=1&exportnowrap=1>
  - One MediaWiki API request exports the current revision of all 134 pages as XML (export format 0.11). The Internet Archive captured it on 2026-11-03.
  - The archive serves the capture with `Content-Encoding: gzip`, and `curl` without `--compressed` (as `xtask starter` runs it) saves the gzip bytes: **13,951,385 bytes, sha256 `149a9a9fe7bfef10ab4d3ae99662051fe90a19243e53735ce85f323db30e0934`**. Two downloads gave the same hash.
  - Decompressed: 42,360,716 bytes, sha256 `9a050eb4a5708f48dc545057f4173006c6e9ee20536827144d0bce625535f5b3`. This is byte-identical to a live export made the same day.
  - The export carries each page's revision id, timestamp and `sha1`. Revisions run from 2008-09-06 to 2022-12-23; nobody has edited the work since.
  - Why not the live API URL: the export's `<generator>` line names the MediaWiki build, which changes weekly, so the live bytes drift even when no page is edited. The archived capture is stable.
  - Why not a Wikimedia dump: the enwikisource dump is several GB to get 42 MB, and dumps are pruned after a few months.
- **origin_url:** `https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew+Henry%27s+Commentary+on+the+Whole+Bible&gaplimit=max&export=1&exportnowrap=1`

### Licence evidence

- The main page carries `{{PD-old}}`: "This work was published before January 1, 1931, and is in the public domain worldwide because the author died at least 100 years ago." Archived as [`starter/evidence/mhc-wikisource-main.htm`](../../starter/evidence/mhc-wikisource-main.htm) (capture `20261103225327` of the rendered page). The tag is also in the main page's wikitext inside the pinned export.
- The talk page's source note is archived as [`starter/evidence/mhc-wikisource-talk.htm`](../../starter/evidence/mhc-wikisource-talk.htm) (capture `20261103225356`).
- Summary and quotes: [`starter/evidence/mhc.md`](../../starter/evidence/mhc.md).

### Coverage

The pinned revisions were fetched one by one (`index.php?oldid=…&action=raw`, 2.5 s apart, descriptive User-Agent, no 429s) and each matched its revision `sha1`. Then they were parsed for chapter headings (`=CHAP. N.=`, or the subpage title `Chapter N`) and section headings (`==verses A-B==`, `== Verse N ==`) and compared with the Standard versification's verse counts (`crates/scripture/src/standard_data.rs`).

- **Books:** 66/66. **Chapters:** 1,189/1,189. **Verse sections:** 4,248.
- **Verses inside a section:** 31,057 of 31,102 (99.86%).
- **The one real gap:** Luke 18:9-43. The `Volume 5/Luke/Chapter 18` subpage holds only verses 1-8, and the Luke page links to it in place of chapter 18.
- **Heading slips, not missing text** (each verse falls inside the chapter's opening text or a neighbouring section): Gen 3:9-10 (no section heading), Gen 13:10-13 (heading typed `Versees 10-13`), Josh 18:1, 1 Sam 25:1, 2 Sam 22:1 (verse 1 quoted and discussed in the chapter introduction), Prov 11:20 (inside the verse 19 section, under a `====Weighty Sayings.====` heading). The converter should key a chapter introduction that quotes verse 1 to the whole chapter, which covers it.
- **Size:** about 39 MB of plain text after stripping markup, which makes this the largest Starter Resource.

### Alternatives checked

- **Wikisource "An Exposition of the Old and New Testament (1828)".** Scan-backed and clean: a Towar & Hogan / Barrington & Haswell printing, with the scans on Commons. But it is far from done. Only Genesis is transcluded into main-namespace pages (50 chapters plus front matter, 79 pages), and the scan transcriptions cover 1,271 of 5,696 pages (vol 1: 408/756, vol 2: 125/930, vol 3: 184/896, vol 4: 119/1,194, vol 5: 217/974, vol 6: 218/946), none marked proofread. Not shippable now. It is the clean replacement if the CCEL lineage below is ever ruled out.
- **CrossWire MHC and MHCC modules.** Both confs cite `TextSource=ccel.org` (#20). Not used.
- **Matthew Henry's Concise, Bible Foundation edition** (the #20 fallback). Not needed because the Complete is complete. Not evaluated further.

## Jamieson-Fausset-Brown

### Source chosen

- **Work:** Robert Jamieson, A. R. Fausset and David Brown, *Commentary Critical and Explanatory on the Whole Bible* (1871).
- **Edition:** the "expanded electronic edition", prepared for the OnLine Bible from text scanned by Woodside Bible Fellowship and edited by Ernie Stefanik (created 1997, last updated 25 January 2001). This is the edition #20 decision 3 names.
- **Host:** Third Millennium Ministries, `https://thirdmill.org/files/english/texts/JFB/`. thirdmill.org has served the HTML files since 2001 and the archive since at least 2012 (Wayback CDX).
- **Pinned file:** <https://web.archive.org/web/20120504083056id_/http://thirdmill.org/files/english/texts/JFB/JFB-UPD3.zip>, **5,573,190 bytes, sha256 `b242392883f5f26a8e0a8cfc4995e9cdb73fa165d9dd754b057ce039c8f258ec`**. The live `https://thirdmill.org/files/english/texts/JFB/JFB-UPD3.zip` had the same hash on 2026-11-03.
- **Contents:** 75 HTML files, 18.9 MB, Latin-1 with CRLF line endings: `JFB00.htm` (contents), `JFB00A`–`JFB00H` (introductions, abbreviations, "About the Electronic Edition"), `JFB01.htm`–`JFB66.htm` (one per Bible book, canonical order).
- Other non-CCEL hosts of the same HTML set exist (clearvisionbiblestudies.com, fritzmiller.com). The clearvisionbiblestudies.com copy has a reworded "About" page, so it is not the editor's text; thirdmill.org's matches the archive byte for byte.

### Licence evidence

- Every page: "This expanded electronic edition of the J-F-B Commentary is in the public domain."
- `JFB00H.htm`, "Public Domain Status": "This expanded edition of the Jameison-Faussett-Brown Commentary is in the public domain and may be freely used and distributed. It was first posted on the Christian Classics Ethereal Library server, at Wheaton College, in December 1997. The links to the Bible Gateway for Scripture references in that copy were added in January 1998 by Harry Plantinga."
- Archived as [`starter/evidence/jfb-about.htm`](../../starter/evidence/jfb-about.htm) (the zip member, LF line endings), plus a fresh capture of the host's page: <https://web.archive.org/web/20261103225457/https://thirdmill.org/files/english/texts/JFB/JFB00H.htm>, byte-identical to the zip member (sha256 `e983846160ef47ab575bb567ccfcfeabd36d108da7a87dcf97e2a88c9daf33a2`).
- Summary and quotes: [`starter/evidence/jfb.md`](../../starter/evidence/jfb.md).

### Coverage

- **Books:** 66/66. **Chapters:** 1,189/1,189.
- **Verse anchors** `<A NAME="Ge1_1">`: 31,102 anchors, 31,100 distinct, none out of range. Ps 78:28 and Acts 15:41 are each anchored twice. Mark 4:21 and Zech 1:2 have no anchor, but their comment is there, under the previous verse's anchor and opened by its own `<B>21.` / `<B>2.` lemma, so every KJV verse has comment text. 19,779 anchors are followed by their own comment; the others sit inside a comment on a verse range (for example `3Jo 1-14`).
- **Scripture references:** 39,598 links (39,354 in the 66 book files) of the form `http://bible.gospelcom.net/bible?version=KJV&passage=3Jo+1:1-14`, so every reference is already parsed into a machine-readable passage.
- **Size:** about 12 MB of plain text.

## Proposed lock entries

For #44 to add to `starter/sources.toml` with the converters. The importer names are proposals; the provenance-gate test checks everything else in these entries (evidence names the sha256, no CCEL host, required fields filled).

<!-- proposed-lock: read by xtask/src/starter/tests.rs -->
```toml
[[source]]
resource_id = "mhc"
importer = "wikisource-mhc"
url = "https://web.archive.org/web/20261103225156id_/https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew+Henry%27s+Commentary+on+the+Whole+Bible&gaplimit=max&export=1&exportnowrap=1"
origin_url = "https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew+Henry%27s+Commentary+on+the+Whole+Bible&gaplimit=max&export=1&exportnowrap=1"
sha256 = "149a9a9fe7bfef10ab4d3ae99662051fe90a19243e53735ce85f323db30e0934"
title = "Matthew Henry's Commentary on the Whole Bible"
abbreviation = "MHC"
language = "en"
edition = "Complete (1706-1721), English Wikisource transcription, 134 pages, revisions to 2022-12-23 (export captured 2026-11-03)"
publisher = "Wikisource"
homepage = "https://en.wikisource.org/wiki/Matthew_Henry%27s_Commentary_on_the_Whole_Bible"
spdx = "LicenseRef-PublicDomain"
attribution = "Matthew Henry, An Exposition of the Old and New Testament (1706-1721), Romans to Revelation completed by other ministers from his notes. Public domain. Transcription from Wikisource, which took its text from the Christian Classics Ethereal Library."
changes = "Converted from Wikisource wikitext to the Open Bible resource format. The commentary text is unchanged; wiki templates, links and page navigation are replaced by plain text and Bible reference spans. Luke 18:9-43 is missing from the source."
provenance = "Wikisource tags the work {{PD-old}} (archived 2026-11-03): published before 1931, author died more than 100 years ago."
evidence = "evidence/mhc.md"

[[source]]
resource_id = "jfb"
importer = "jfb-html"
url = "https://web.archive.org/web/20120504083056id_/http://thirdmill.org/files/english/texts/JFB/JFB-UPD3.zip"
origin_url = "https://thirdmill.org/files/english/texts/JFB/JFB-UPD3.zip"
sha256 = "b242392883f5f26a8e0a8cfc4995e9cdb73fa165d9dd754b057ce039c8f258ec"
title = "Commentary Critical and Explanatory on the Whole Bible"
abbreviation = "JFB"
language = "en"
edition = "Jamieson, Fausset and Brown (1871), expanded electronic edition by Ernie Stefanik from Woodside Bible Fellowship's scan (JFB-UPD3.zip, last updated 2001-01-25)"
publisher = "Third Millennium Ministries (host)"
homepage = "https://thirdmill.org/files/english/texts/JFB/JFB00.htm"
spdx = "LicenseRef-PublicDomain"
attribution = "Robert Jamieson, A. R. Fausset and David Brown, Commentary Critical and Explanatory on the Whole Bible (1871). Expanded electronic edition prepared from text scanned by Woodside Bible Fellowship, edited by Ernie Stefanik. This expanded electronic edition is in the public domain."
changes = "Converted from HTML to the Open Bible resource format. The commentary text is unchanged; page navigation and the Bible Gateway links are dropped, and each linked reference is kept as a Bible reference span."
provenance = "The edition's own notice inside the pinned zip (JFB00H.htm): in the public domain and may be freely used and distributed."
evidence = "evidence/jfb.md"
```

## Recommended importer shape for #44

Both converters write a Commentary through `ResourceBuilder`, Keyed by Passage in the Standard versification. Both works follow KJV numbering, so no Verse map is needed. One text unit per Passage-keyed section, as #44 asks.

**`wikisource-mhc`** (MediaWiki XML export, gzip):

1. Gunzip if the bytes start with `1f 8b`, then stream-parse `<page>` elements (`<title>`, `<ns>`, `<revision><id>`, `<text>`). Skip `#REDIRECT` pages and the main and volume index pages.
2. Map the page title to a Bible book from the segment after `Volume N/` (`First Samuel`, `Song of Solomon`, …). Join split pages in title order: `Psalms`, `Psalms second part`, `Psalms third part`; `Isaiah`, `Isaiah part2`; `Acts`, `Acts Part2`; `Matthew/Matthew part1|2`; `John/John part1|2`. Book subpages `…/Chapter N` set the chapter; a book page's `* [[/Chapter 18|…]]` line stands for that subpage.
3. Units: text under `=Preface=` → book introduction, keyed to the whole book. Text between `=CHAP. N.=` and its first section → chapter introduction, keyed to the whole chapter. Each `==verses A-B==` / `== Verse N ==` → one unit keyed to `Book N:A-B`. Accept the typo `Versees`. The `====Title (B. C. 1857.)====` lines become headings inside the unit.
4. Inline markup: `''…''` italics; `{{small-caps|…}}` small caps; `{{sup|N}}` verse numbers inside quoted Scripture; `<poem>` poetry lines; `<ref>` notes (32); `<br>` paragraph breaks. Drop `{{header}}` and the navigation templates (`{{MHC Genesis}}` …).
5. `[[Bible (King James)|Eph. iv. 21]]` (63,492 links) becomes a Bible reference span parsed from the display text. Chapters there are lower-case Roman numerals, and lists like `John iv. 21, &c.; 1 Tim. ii. 8` occur, so the reference parser needs a Roman-chapter mode. Unparsed ones stay plain text and are counted in the build report.
6. Unit ids from the Passage (e.g. `Gen.24.1-9`), so they stay stable when a revision changes the text.

**`jfb-html`** (zip of 75 HTML files):

1. Read `JFB01.htm`–`JFB66.htm` as Latin-1 (book = file number in canonical order). Use `JFB00A`–`JFB00G` as front matter only if #44 wants it; keep `JFB00H` (the notice) out of the text.
2. Split at `<A NAME="{abbr}{c}_{v}">` anchors, ignoring a repeated anchor, and also at a `<B>N.` lemma whose verse has no anchor (Mark 4:21, Zech 1:2). The OnLine Bible abbreviations, in canonical order: `Ge Ex Le Nu De Jos Jud Ru 1Sa 2Sa 1Ki 2Ki 1Ch 2Ch Ezr Ne Es Job Ps Pr Ec So Isa Jer La Eze Da Ho Joe Am Ob Jon Mic Na Hab Zep Hag Zec Mal Mt Mr Lu Joh Ac Ro 1Co 2Co Ga Eph Php Col 1Th 2Th 1Ti 2Ti Tit Phm Heb Jas 1Pe 2Pe 1Jo 2Jo 3Jo Jude Re`. A section opener `<A HREF="…passage=3Jo+1:1-14">3Jo 1-14</A>.` followed by a small-caps title gives a Passage-keyed section heading. Each anchor's paragraphs (`<B>1. lemma</B>--comment`) form the unit for that verse, or for the range in the opener.
3. Every `bible.gospelcom.net/bible?…passage=` link becomes a Bible reference span from its `passage` parameter (already OnLine Bible abbreviations, e.g. `Pr 19:9`), so no prose parsing is needed. Drop the URL itself.
4. Markup: `<B>`, `<I>`, `<FONT SIZE=-1>` small caps, `&nbsp;` indents, `<P>`/`<BR>` paragraphs. Drop the navigation table and the public-domain banner (the notice is shown in About instead).

**Lock and gate:** add `wikisource-mhc` and `jfb-html` to `lock::IMPORTERS` and the `build_one` match, move the two entries above into `starter/sources.toml`, add `mhc` and `jfb` to `the_real_lock_passes_the_provenance_gate`, and delete `the_proposed_commentary_lock_entries_pass_the_provenance_gate` together with the `proposed-lock` block here.

## Risks

- **MHC's text came from CCEL.** The Wikisource talk page names CCEL as the source, and a 2023 note there says CCEL's edition "is from an unknown source and therefore cannot be further proofread". Decision 3 refuses CCEL-hosted *files* because CCEL's site terms restrict republishing. We use none, and Wikisource tags the work public domain. US law gives no copyright to a faithful transcription of a public-domain text, and CCEL's terms bind people who download from ccel.org, not later copies. Still, a strict reading of "not from CCEL" would refuse it. Mitigations: credit CCEL in the Attribution, send the courtesy note decision 3 already plans, and keep the scan-backed 1828 Wikisource transcription as the clean replacement as it fills in. If the maintainer rules the lineage out, MHC **drops** from the Starter library (no clean, complete alternative exists today) and stays available through Import.
- **JFB was also first posted on CCEL**, and CCEL's director added the Bible Gateway links. The editor's notice puts the edition in the public domain and allows free use and distribution, so the CCEL history does not limit it. The pinned bytes come from thirdmill.org, and the converter drops those links.
- **thirdmill.org has no stated terms for hosted texts.** Its footer, "Copyright 2011-2026 … All rights reserved", covers its own pages. Our licence rests on the edition's notice inside the zip.
- **Wikisource's site-wide licence** is CC BY-SA 4.0 for original contributions. The MHC pages hold Henry's text plus formatting templates. We treat the formatting as non-creative and drop it; the commentary text is public domain.
- **Pinned gzip bytes.** The MHC pin is the gzip stream the Internet Archive serves. If the archive ever served it decompressed, the hash would change and the build would fail loudly (no silent drift). The decompressed sha256 above identifies the content either way.
- **Size budget.** The Starter set is 183.5 MB of 400 MB (#38). MHC (~39 MB of text) and JFB (~12 MB) plus their Search index caches might add roughly 100–150 MB. That fits, but MHC will be the largest Resource. #44's build report will give the real number.
- **Quality.** Neither text can be checked against page scans (the CCEL source of MHC is unknown; the JFB scan is Woodside's). The known MHC gap (Luke 18:9-43) should be listed in the About "Changes we made" note, as in the proposed entry.

## How to reproduce

```sh
UA='OpenBibleSoftware-research/0.1 (https://github.com/inhesrom/openbiblesoftware)'
# MHC pages and their lengths (one request; wait 2 s or more between Wikisource requests)
curl -A "$UA" 'https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew%20Henry%27s%20Commentary%20on%20the%20Whole%20Bible&gaplimit=500&prop=info|revisions&rvprop=ids|timestamp|sha1&format=json&formatversion=2'
# The pinned files
curl -L -o mhc.xml.gz 'https://web.archive.org/web/20261103225156id_/https://en.wikisource.org/w/api.php?action=query&generator=allpages&gapprefix=Matthew+Henry%27s+Commentary+on+the+Whole+Bible&gaplimit=max&export=1&exportnowrap=1'
curl -L -o jfb.zip 'https://web.archive.org/web/20120504083056id_/http://thirdmill.org/files/english/texts/JFB/JFB-UPD3.zip'
sha256sum mhc.xml.gz jfb.zip
gunzip -c mhc.xml.gz | sha256sum
```
