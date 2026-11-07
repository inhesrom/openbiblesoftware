# Parallel passages data (#212 spike)

What open data can fill the Passage Guide's **Parallel passages** section: a Gospel harmony
(the accounts of one event in Matthew, Mark, Luke and John) and the parallels between Samuel /
Kings and Chronicles. Checked live from this project's build host on 2026-11-07. The CCEL rule
(#20: no CCEL-hosted files, even through an archive) applies.

## Candidates

| Source | Licence | Coverage | Shape | Verdict |
|---|---|---|---|---|
| **BSB USFM `\r` lines** (`bsb_usfm.zip`, already pinned for the `bsb` Starter entry, sha256 `f0c56438…5f87`; the live file at <https://bereanbible.com/bsb_usfm.zip> had the same hash on 2026-11-07) | Public domain (dedicated 2023-04-30; [evidence](../../starter/evidence/bsb-parallels.md)) | Whole Bible: 1,325 `\r` lines under `\s1` headings with 2,033 references. Gospels (Matthew 140 lines, Mark 94, Luke 125, John 41), Samuel/Kings ↔ Chronicles (1 Sam 12, 2 Sam 28, 1 Kgs 38, 2 Kgs 55, 1 Chr 40, 2 Chr 76), Psalms ↔ Samuel/Chronicles (64), the Law's repeated laws (Exod 68, Lev 37, Num 40, Deut 50), Isaiah 36-39 ↔ 2 Kings, and the Old Testament passages the New quotes | Machine-readable: each section heading lists its parallels as USFM `\ref` targets (`MRK 1:9-11`) | **Chosen** |
| Wikipedia, "Gospel harmony", *Parallel harmony* table | CC BY-SA 4.0 (and GFDL) | Gospels only: 166 events with their Matthew, Mark, Luke and John Passages | Wikitext table | Not chosen: Gospels only (no Kings/Chronicles), share-alike, and its order is partly taken from a copyrighted harmony (Cox & Easley, *HCSB Harmony of the Gospels*, 2006) |
| A. T. Robertson, *A Harmony of the Gospels* (1922); J. A. Broadus, *A Harmony of the Gospels* (1893) | Public domain in the US | Gospels | The usual machine-readable text is CCEL's (refused). Internet Archive has page scans only (`harmonyofgospels0000robe_a5r6`, `harmonyofgosp00broa`): OCR plus hand transcription of some 180 sections | Not chosen (no clean source); a possible later harmony |
| W. D. Crockett, *A Harmony of the Books of Samuel, Kings and Chronicles* (1897) | Public domain | Samuel/Kings ↔ Chronicles | Scans only (`harmonyofbooksof0000will`) | Not chosen: the BSB already gives these parallels |
| CrossWire SWORD catalog (`mods.d.tar.gz`, fetched 2026-11-07) | — | — | No harmony or parallels dataset. `TFG` (McGarvey & Pendleton, *The Fourfold Gospel*, public domain) is a Commentary whose text interleaves the Gospels, not a table; `TSK` is Cross-references | Not usable as data |
| OpenBible.info Cross References (Starter `openbible-xrefs`) | CC BY 4.0 | Whole Bible | Voted links between Verses | Not parallels: related Verses, not accounts of the same event |
| GitHub search ("gospel harmony", "synoptic parallels", "parallel passages bible") | — | — | A handful of web apps with no or a custom licence (`forlack/gospel-harmony`: "other") | Nothing usable |

## Decision

Ship the BSB's own section references as a new Starter Cross-reference set, `bsb-parallels`
(ADR 0014), read from the same pinned zip as the `bsb` Bible. One download, already covered by
archived licence evidence, no share-alike, and the parallels a lay reader expects in both the
Gospels and the histories.

The BSB lists parallels per section, and its sections are not equally fine on both sides: Mark
1:1-11 ("The Mission of John the Baptist") lists Matthew 3:1-17, while Matthew 3:13-17 ("The
Baptism of Jesus") lists Mark 1:9-11. The section reads each section both ways and keeps the
narrowest evidence (`obs_engine::parallel_passages`), so Mark 1:9-11 lists Matthew 3:13-17, Luke
3:21-22 and John 1:29-34, and 2 Samuel 24 lists 1 Chronicles 21:1-6, 7-13, 14-17 and 18-30 (in
all, 1 Chronicles 21) beside Exodus 30:11-16.

The BSB's parallels include some that are thematic rather than the same account (John 3 beside
Genesis 22; Matthew 5:3-12 beside Psalm 1). They are kept as the BSB gives them, with the
heading that pairs them shown on every row.

## Numbers from the build (2026-11-07)

`cargo xtask starter`: `bsb-parallels` r1, 0.2 MB, 1,324 sections with 2,032 parallel
Passages. One reference is dropped: `Joshua–Malachi` beside Hebrews 11:30-40 (it spans Bible
books), which leaves that section out. The BSB numbers every verse as the Standard
versification does (its Verse map is the identity but for the Psalm 1 and 107 headings), so the
references are read in the Standard versification directly.
