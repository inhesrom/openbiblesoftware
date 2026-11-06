# Reading plan templates: sources and licences (#147)

Spike for [#147](https://github.com/inhesrom/openbiblesoftware/issues/147), run on
2026-11-06 with network access from the build host. Question: which open data can the
built-in Reading plan templates use, and what licence does each carry?

## Result

| Template | Source | Licence | Shipped as |
|---|---|---|---|
| Bible in a year (canonical) | Generated: Gen–Rev divided by Verse count | MIT OR Apache-2.0 (ours) | `bible-canonical.toml` (a rule, not a day list) |
| New Testament in 90 days | Generated: Matt–Rev | MIT OR Apache-2.0 (ours) | `nt-90-days.toml` |
| Psalms and Proverbs monthly | Generated: Psalms and Proverbs over 31 days | MIT OR Apache-2.0 (ours) | `psalms-proverbs.toml` |
| Bible in a year (chronological) | **Our own arrangement** (see below) | MIT OR Apache-2.0 (ours) | `bible-chronological.toml` |
| Bible in a year (M'Cheyne) | Robert Murray M'Cheyne, *Daily Bread* (1842) | Public domain | `mcheyne.toml` (365 day rows) |

All five are in `crates/engine/assets/reading-plans/` with `ATTRIBUTION.toml` entries, and
appear in Help → Credits under data built into the app.

## M'Cheyne

- **Work**: "Daily Bread, being a Calendar for Reading through the Word of God in a Year",
  dated St Peter's, Dundee, 30 December 1842; M'Cheyne died in 1843, so it is public domain
  everywhere. It is printed in Andrew A. Bonar's *Memoir and Remains of the Rev. Robert
  Murray M'Cheyne*. Scans checked on the Internet Archive:
  [dli.bengal.10689.19457](https://archive.org/details/dli.bengal.10689.19457) (1857
  edition; the calendar's introduction and directions at the scan's pp. 567–571, the table
  from p. 572) and
  [in.ernet.dli.2015.53310](https://archive.org/details/in.ernet.dli.2015.53310). The
  introduction confirms the shape the test checks: "The Old Testament once, the New
  Testament and Psalms twice." The scans' OCR of the table itself is unusable, so the table
  was not taken from it.
- **Transcription**: three independent machine-readable transcriptions were compared day by
  day (365 days × 4 readings):
  - [willswire/mcheyne](https://github.com/willswire/mcheyne) `App/PlanConstants.swift`
    (Apache-2.0),
  - [speric/mcheyne-api](https://github.com/speric/mcheyne-api) `plan.json`,
  - [cchmb/mcheyne-reading-plan](https://github.com/cchmb/mcheyne-reading-plan)
    `plugin.php`.

  They agree on every reading. The only 19 differing days differ in notation: speric keeps
  the calendar's "Luke 1:39ff" and "Ex 11-12:21" forms and "Jeremiah 36 & 45" where the
  other two write "Luke 1:39-80", "Exodus 11:1-12:21" and "Jeremiah 36,45". Our file writes
  the explicit ends (a fact of the KJV chapter lengths, not anyone's expression), names the
  one-chapter books whole ("Jude", since our reference parser reads "Jude 1" as its first
  verse) and splits "Jeremiah 36; Jeremiah 45". A list of chapter references from a public
  domain calendar is not copyrightable; the file is credited to M'Cheyne as public domain.
- **Check**: `mcheyne_reads_the_old_testament_once_and_the_new_testament_and_psalms_twice`
  counts every Standard Verse: each Old Testament Verse outside the Psalms is read exactly
  once, each Psalms and New Testament Verse exactly twice, and nothing else.

## Chronological orders: licence check

Searched GitHub (`chronological bible reading plan`, `bible reading plans json`) and the
usual publisher plans:

- Open-source data with a licence: `BennyThadikaran/bible` (GPL-3.0), `albert429/ChronosBible`
  (AGPL-3.0). Copyleft, not compatible with shipping inside an MIT/Apache-2.0 app.
- Repositories without a licence (all rights reserved by default): `jkeown/read-bible`,
  `Amgad-Fouad/Bible-Chronological-Reading-Plan`, `slyfox29/1-Year-Chronological-Bible-Reading-Plan`,
  `gavin-winslow/chrono-kjv`, `Joelquan/bible-reading-plan`, `elijahlovold/bible-plan-generator`,
  `khornberg/readingplans` and others; several are transcriptions of publishers' plans.
- Publisher and ministry plans (One Year Chronological Bible, The Bible Recap, Blue Letter
  Bible's and YouVersion's chronological plans): copyrighted, no open licence.

**Decision**: no published chronological plan is copied. `bible-chronological.toml` is our
own arrangement of whole books and chapter ranges, written for this project from placements
that are common knowledge in Bible handbooks: Job after Genesis 11; Psalm 90 with Moses;
1 Chronicles 1–9 before Samuel; Chronicles interleaved with Samuel and Kings by reign; the
Psalms in their five books beside David, Solomon, Hezekiah and the return; Song, Proverbs and
Ecclesiastes with Solomon; Obadiah, Joel, Jonah, Amos, Hosea, Micah and Isaiah in the reigns
they name; Nahum, Zephaniah, Habakkuk, Jeremiah, Lamentations, Ezekiel and Daniel around the
exile; Haggai and Zechariah with Ezra 1–6, then Esther, Ezra 7–10, Nehemiah and Malachi; the
birth narratives, then the Gospels; Acts with the letters placed in its journeys
(James, Galatians, Thessalonians, Corinthians, Romans, then the prison and pastoral letters,
Peter, Hebrews, Jude, John's letters and Revelation). Its days are cut by Verse count like
the canonical plan. The test
`bible_in_a_year_chronological_reads_every_chapter_exactly_once` checks that every Verse is
read exactly once.

## Generated templates

The canonical, New Testament and Psalms and Proverbs plans are rules, not day lists: their
files name the Passages and the number of days, and the app divides them by Verse count
(`docs/dev/reading-plans.md`). They are the project's own work.
