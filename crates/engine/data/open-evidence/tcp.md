# Evidence: Text Creation Partnership texts (`tcp-*`)

This file is the licence evidence for every Text Creation Partnership (TCP) text in the Open
catalogue: the ten hand-written entries of `open-catalogue.toml` (table below) and every row of
the TCP source collection, `data/collections/tcp-books.tsv` (ADR 0024).

- **Editions offered:** ten TEI texts keyboarded and encoded by the Text Creation Partnership
  (TCP) from early printed books: nine from EEBO-TCP (Early English Books Online), one from
  Evans-TCP (Evans Early American Imprints). The table below lists them.
- **Download:** each text's own GitHub repository under
  <https://github.com/textcreationpartnership>, where TCP publishes its texts, as a raw file at a
  fixed commit. The files are not re-hosted.
- **Pinned:** each file's sha256, below. The URL names a commit, so the file does not change; any
  other bytes are refused.
- **Checked on 2026-11-21:** every file downloaded from the URL below, matched its sha256 and
  imported as a Book (the ignored live test
  `live_every_open_catalogue_entry_downloads_checks_and_imports`).
- **Archived licence page:** <https://web.archive.org/web/20260314193425/https://textcreationpartnership.org/faq/>
  (Internet Archive capture of TCP's FAQ, <https://textcreationpartnership.org/faq/>). It says:

  > When will the texts be freely available? All of the texts are now freely available. […]
  > Evans-TCP was released to the public on June 30, 2014 EEBO-TCP Phase I was released to the
  > public on January 1, 2015 EEBO-TCP Phase II was released to the public on August 1, 2020.

  and of Phase I: "This set of texts was released to the public with no restrictions on use
  whatever, as of January 1, 2015." TCP's EEBO-TCP page, captured at
  <https://web.archive.org/web/20251206131037/https://textcreationpartnership.org/tcp-texts/eebo-tcp-early-english-books-online/>,
  says: "As of August 1, 2020, all texts produced under EEBO Phase I and Phase II are freely
  available to the public."

TCP's catalogue (`TCP.csv` in <https://github.com/textcreationpartnership/Texts>) still marks
two of these texts (`A96226`, `A77614`) "Restricted": its status from before Phase II's release
on 1 August 2020. Each of those two files carries the Phase II waiver below.

## Licence

Each file states its own licence in its TEI header (`<availability>`). Each statement has a
code, which `tcp-books.tsv` records for each text (`obs_engine::open_catalogue::TCP_CC0_STATEMENTS`
holds them, whitespace collapsed as the check reads them). The Phase I texts and the
Evans texts say (`cc0-phase1`):

> This keyboarded and encoded edition of the work described above is co-owned by the institutions providing financial support to the Early English Books Online Text Creation Partnership. This Phase I text is available for reuse, according to the terms of Creative Commons 0 1.0 Universal . The text can be copied, modified, distributed and performed, even for commercial purposes, all without asking permission.

The Phase II texts say (`cc0-waiver`):

> To the extent possible under law, the Text Creation Partnership has waived all copyright and related or neighboring rights to this keyboarded and encoded edition of the work described above, according to the terms of the CC0 1.0 Public Domain Dedication (http://creativecommons.org/publicdomain/zero/1.0/). This waiver does not extend to any page images or other supplementary files associated with this work, which may be protected by copyright or other license restrictions. Please go to https://www.textcreationpartnership.org/ for more information about the project.

SPDX: `CC0-1.0`. Licence class: free-to-share.

The waiver covers TCP's keyboarded and encoded edition; the works themselves were printed
between 1630 and 1741 and are in the public domain. The page images, which are not TCP's, are
not used.

## The TCP collection (`tcp-books.tsv`)

`cargo xtask tcp-books` writes one row per text: its TCP id, the commit of its repository
(`https://github.com/textcreationpartnership/<id>`), the sha256 and size of `<id>.xml` at that
commit, and what its header says (language, date, author, title), with the code of its
statement. A text is added only when its `<availability>` is, whitespace aside, exactly one of
the two statements above, and it imports as a Book. Any other statement, or none, leaves it out.
Each row becomes an entry downloading
`https://raw.githubusercontent.com/textcreationpartnership/<id>/<commit>/<id>.xml`, refused
unless its sha256 matches. The rows are the per-text record; this file is the evidence for all
of them, and its archived FAQ capture is every row's `evidence_url`.

TCP's catalogue status ("Free" or "Restricted") is not read: "Restricted" marks the Phase II
texts withheld until 1 August 2020, and every one of them now carries the waiver.

## Hand-written entries

| Entry | Title | TCP id | Download | Bytes | sha256 | Licence in the file |
|---|---|---|---|---|---|---|
| `tcp-owen-mortification-1668` | Of the Mortification of Sin in Believers | `A53715` | <https://raw.githubusercontent.com/textcreationpartnership/A53715/588ac23f659abf650a1391e7a67517a48b4b7df8/A53715.xml> | 349,649 | `4603d4fd73838e698e523487eebb04841f242be20f61b01c3a510bfa35cd661d` | Phase I statement |
| `tcp-westminster-confession-1646` | The Humble Advice of the Assembly of Divines concerning a Confession of Faith | `A96226` | <https://raw.githubusercontent.com/textcreationpartnership/A96226/71079803327937df5a05ed41da65499217b40e6e/A96226.xml> | 102,440 | `6134a92e34e8fd7bfcaacbfbc9ecc30eec4b35bedbb37684d8f22d050fb7d95b` | Phase II waiver |
| `tcp-bunyan-pilgrims-progress-1678` | The Pilgrim's Progress | `A30170` | <https://raw.githubusercontent.com/textcreationpartnership/A30170/0b06ff409f830ec0e14e5efdeb05782c9bbfea41/A30170.xml> | 578,078 | `14c3c96cab3e5a57cb088d7b58c98ff2d40b05dffe25ab3c42d964a814896e5c` | Phase I statement |
| `tcp-baxter-reformed-pastor-1656` | Gildas Salvianus: The Reformed Pastor | `A26932` | <https://raw.githubusercontent.com/textcreationpartnership/A26932/915118d4acbb654272e79c67bde04160cb1ea3b8/A26932.xml> | 1,077,411 | `70f22af8b839874e796ac630b195e961218feb0de2a4a2005240bedf99100258` | Phase I statement |
| `tcp-sibbes-bruised-reed-1630` | The Bruised Reed and Smoking Flax | `A12171` | <https://raw.githubusercontent.com/textcreationpartnership/A12171/a54ed9aa0dd1ff39911a34002cd54669547ec099/A12171.xml> | 339,148 | `89497fe6c95628ba4634234598ae16198da47a74b64c2d239ca1fdee50855406` | Phase I statement |
| `tcp-brooks-precious-remedies-1658` | Precious Remedies against Satan's Devices | `A77614` | <https://raw.githubusercontent.com/textcreationpartnership/A77614/8755b893db9a5682c1bd54dcba40148f57100fbb/A77614.xml> | 824,260 | `0d7babe825595733c9272cb350c7bdbab5e352dd40d0548a78a40fa14daf0ed7` | Phase II waiver |
| `tcp-alleine-alarm-1672` | An Alarm to Unconverted Sinners | `A26682` | <https://raw.githubusercontent.com/textcreationpartnership/A26682/7eae889a57e295cf6ab34b14cf76c6a76066d000/A26682.xml> | 463,900 | `7f289f58ed65af6724de518379272165282de6bd767f9707da18e267cc68d24c` | Phase I statement |
| `tcp-flavel-saint-indeed-1668` | A Saint Indeed | `A39682` | <https://raw.githubusercontent.com/textcreationpartnership/A39682/619de5b6004f6bfcc56b659ed12f760e58b6d62d/A39682.xml> | 387,979 | `b55cbc7cc7da687e5e83721b16ed8e5f8585aad71ee0fad676926d9ca2ed6a49` | Phase I statement |
| `tcp-edwards-sinners-1741` | Sinners in the Hands of an Angry God | `N03833` | <https://raw.githubusercontent.com/textcreationpartnership/N03833/02f4e8db11f49192eea22ce669fd55b2cb10f8ea/N03833.xml> | 55,061 | `f137afaaf5a596a34a42a2fe0edacc5280301e6da41c82838986b9cba8be23ed` | Phase I statement |
| `tcp-bunyan-grace-abounding-1666` | Grace Abounding to the Chief of Sinners | `A30143` | <https://raw.githubusercontent.com/textcreationpartnership/A30143/180725de9440db6afe9bfb7283629059942030d7/A30143.xml> | 275,294 | `6d927ab6622091da3b311a9615a019788695f870b8f6ba44012e2a9dfe744c7c` | Phase I statement |
