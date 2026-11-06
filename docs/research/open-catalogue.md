# Open catalogue and licence classes (#134)

**Question:** Which free and openly licensed resources can users get beyond the Starter
library? Which of them are not SWORD modules and need our own importer, and how should Get
Resources sort SWORD's free-text `DistributionLicense` values into "free to share" and "free for
personal use"?
**Date:** 2026-11-06. Every live check below was run from this host on that day: all 11
repositories on CrossWire's master list, eBible.org's `translations.csv`, the publishers'
licence files, and Internet Archive captures.
**Outcome:** a licence filter in Get Resources (`obs_engine::licence`), and the Open catalogue
(`crates/engine/data/open-catalogue.toml`) with 11 eBible.org USFM Bibles. Their evidence is in
`crates/engine/data/open-evidence/`. The decision is recorded in
[ADR 0008](../adr/0008-open-catalogue-downloads-from-the-publisher-checked-by-pin-or-licence-notice.md).

## 1. SWORD `DistributionLicense` values (live)

Read from each repository's `mods.d.tar.gz` (CrossWire, CrossWire Beta, CrossWire Attic,
CrossWire Wycliffe, Bible.org, Xiphos, eBible.org, Deutsche Bibelgesellschaft, Lockman, IBT and
STEP Bible): **2,411 confs**, 2,374 of them with a `DistributionLicense`, in **66 distinct
values**. The ten most common values:

| Count | Value | Class |
|---:|---|---|
| 848 | `Creative Commons: by-nc-nd` / `BY-NC-ND 4.0` | Free for personal use |
| 428 | `Public Domain` (and `Public domain.`, `public domain`) | Free to share |
| 292 | `Creative Commons: by-sa` | Free to share |
| 250 | `Copyrighted; Permission to distribute granted to eBible.org` | Free for personal use |
| 151 | `Copyrighted; Permission to distribute granted to CrossWire` | Free for personal use |
| 101 | `Copyrighted; Free non-commercial distribution` | Free for personal use |
| 93 | `Creative Commons: by-nd` | Free for personal use |
| 59 | `Creative Commons: BY-SA 4.0` | Free to share |
| 38 | `Copyrighted` / `Copyrighted.` | Unclear |
| 15 | `GPL` | Free to share |

Every one of the 66 values is listed with its class in
`crates/engine/src/licence.rs` (`LIVE_VALUES`, test `every_live_distribution_license_has_its_class`).
The rules:

- **Restrictions win.** Any non-commercial wording (`nc`, "non-commercial", "noncommercial",
  "non commercial", Polish "niekomercyjne") or a Creative Commons `nd` makes the work **Free for
  personal use**, whatever else the value says.
- **Free to share:** Public Domain (including SPDX `LicenseRef-PublicDomain`), CC0, a Creative
  Commons licence with `by`/`sa`/"Attribution"/"Atribuição", or GPL ("General public license").
- **Free for personal use:** "permission to distribute", "distribution permitted", "distribution
  rights granted", "freely distributable".
- **Unclear:** everything else. That includes "Copyrighted" with nothing more, publisher names
  with a year, "See README file", "Unrestricted", GFDL, and an empty value. Unclear is never
  shown as free.

Run against the live catalog as Get Resources builds it (2,248 listings after dropping duplicates,
locked modules and general books; `live_free_to_share_over_the_live_catalog_keeps_only_free_rows`):
**716 Free to share, 1,453 Free for personal use, 79 Unclear.** 32 of the Unclear rows have no
`DistributionLicense` at all, and 29 say only "Copyrighted".

Two classes are left Unclear on purpose and would need a person to decide: "Unrestricted" (3
Xiphos modules) and GFDL (2). The ticket's Free to share list is PD, CC0, CC BY, CC BY-SA and GPL,
so neither is on it.

## 2. Bibles

| Candidate | Licence (primary source) | Source URL | Importer | Recommendation |
|---|---|---|---|---|
| **eBible.org USFM catalogue** | Per translation. `translations.csv` has 1,552 rows: `Redistributable=True` on 1,295, and the `Copyright` column says public domain on 111. eBible's own SWORD repository gives the licence class: of its 1,530 modules, 102 are PD and 290 CC BY-SA (**392 Free to share, in 248 languages**), 736 BY-NC-ND, 93 BY-ND, 250 permission-to-distribute and 59 non-commercial | <https://ebible.org/Scriptures/translations.csv>, `https://ebible.org/Scriptures/<id>_usfm.zip` | `usfm-zip` (**built**) | **Listed now:** 11 Bibles (§6). Grow the list by hand, one evidence file each. Do not import `translations.csv` wholesale, because its columns do not state the licence. The zips are regenerated on every eBible build, so entries check the `copr.htm` inside instead of a pin |
| **Literal Standard Version** | CC BY-SA 4.0 (`copr.htm`: "Creative Commons Attribution Share-Alike license 4.0") | `englsv_usfm.zip` | `usfm-zip` | **Listed** (`lsv`) |
| **Open English Bible** | CC0. openenglishbible.org: "The OEB is under a Creative Commons Zero licence - that is, it is in the public domain" ([capture](https://web.archive.org/web/20260813070837/https://openenglishbible.org/)); eBible says Public Domain | `engoebus_usfm.zip` (also `engoebcw`, Commonwealth spelling) | `usfm-zip` | **Listed** (`oeb-us`): the NT and the 17 OT books finished so far |
| **unfoldingWord ULT / UST** | CC BY-SA 4.0 (`LICENSE.md` in both repositories). "unfoldingWord®" is a registered trademark, and using the name or logo needs written permission; the Attribution line they ask for is allowed | <https://git.door43.org/unfoldingWord/en_ult>, `/en_ust` | `usfm-zip` would read a release archive, but the USFM is word-aligned (`\zaln-s` milestones, `\w` with `x-*` attributes), so the importer must drop the alignment cleanly first | **Later.** Test the alignment markup against our USFM importer, then pin a tagged release archive (stable, so sha256-pinned) |
| eBible WEB/KJV/ASV/BSB | PD | — | — | Already in the Starter library |

## 3. Original-language texts

| Candidate | Licence | Source URL | Importer | Recommendation |
|---|---|---|---|---|
| **OSHB / WLC with morphology** | Lemma and morphology CC BY 4.0, WLC text PD (`openscriptures/morphhb` README and LICENSE.md). CrossWire `OSHB` 2.1.4: "Creative Commons: BY 4.0" | <https://github.com/openscriptures/morphhb> | Not needed | **Use SWORD:** already in the Featured Hebrew & Greek group (#133), and Free to share |
| **SBLGNT** | **CC BY 4.0** since the 2022-12-19 public release (<https://sblgnt.com/license/>). The CrossWire `SBLGNT` 1.3.1 conf still says "Copyrighted; Free non-commercial distribution", which is stale, so the licence filter shows it as Free for personal use | sblgnt.com and its text files | New `sblgnt-text` importer (verse-per-line text) | **Next importer.** A Free to share critical text, and the base for MorphGNT. Pin a tagged release |
| **MorphGNT** | Morphology CC BY-SA 3.0; the text is the SBLGNT (CC BY 4.0; the README still links the old EULA). CrossWire `MorphGNT` 2.0.1: "Copyrighted; Permission to distribute granted to CrossWire" | <https://github.com/morphgnt/sblgnt> | New `morphgnt` importer (tab-separated, one word per line, Robinson-style parsing) | **High value.** It would give a Free to share, Morphology-tagged Greek NT; today the SWORD copy filters as personal use. Pin a commit |
| **Nestle 1904** | Text PD. Morphology in `biblicalhumanities/Nestle1904/morph` carries a CC0 badge; the other folders have their own READMEs. CrossWire `Nestle1904` 1.3: PD, but no Strong's or Morphology filters | <https://github.com/biblicalhumanities/Nestle1904> | New importer for its morph files | **Later.** The PD text is in Get Resources already. The tagged edition needs the importer, plus a per-folder licence check |
| **Byzantine text** | `byztxt/byzantine-majority-text`: "All the code and text contained in this folder is in the Public Domain". CrossWire `Byz` (2013 edition): CC BY-NC-SA 4.0 (personal use). Attic `Byz` (2005): PD | <https://github.com/byztxt/byzantine-majority-text> | New importer (its CSV/text with parsing) | **Later.** The repository is the Free to share source; the current CrossWire module is not |

## 4. Lexicons

| Candidate | Licence | Source URL | Importer | Recommendation |
|---|---|---|---|---|
| **Thayer** (1889) | PD by age. No digital edition with a clear licence found. No repository on the master list carries a Thayer module (all 11 checked) | Page scans: Internet Archive `cu31924075613939` (1889) | OCR and an editing project, not an importer | **Defer.** Abbott-Smith, Strong's and TBESG (Starter) cover the same ground |
| **Dodson** | PD: "This lexicon, in all of its forms, is in the public domain" (`biblicalhumanities/Dodson-Greek-Lexicon` README). CrossWire `Dodson`: PD | <https://github.com/biblicalhumanities/Dodson-Greek-Lexicon> | Not needed | **Use SWORD:** Free to share in Get Resources |
| **Full BDB** | OSHB HebrewLexicon: markup CC BY 4.0, BDB text PD | openscriptures/HebrewLexicon | — | Already the Starter `bdb` |
| **LSJ** | PerseusDL/lexica: CC BY-SA ("Reuse must credit the Perseus Digital Library"). CrossWire `MLStrong` (Intermediate LSJ keyed to Strong's): CC BY-SA 4.0. Beta `LiddellScott`: CC BY-NC-SA | <https://github.com/PerseusDL/lexica> | New TEI importer for the full LSJ (large) | **Use SWORD `MLStrong` now** (Free to share). The full LSJ is a later importer |

## 5. Dictionaries, commentaries, cross-references and places

The CCEL rule from #20 still applies: none of this comes from ccel.org, even through an archive.
The live confs show two SWORD modules built from CCEL: **Nave's** (`TextSource=https://ccel.org/ccel/n/nave/bible.xml`,
History 3.0, 2021) and **Calvin's Commentaries** (`TextSource=http://www.ccel.org/`). Neither
is a candidate for the Starter library or the Open catalogue. Get Resources still lists them as
CrossWire distributes them; the rule covers what we ship and curate.

| Candidate | Licence (live conf) | Source | Importer | Recommendation |
|---|---|---|---|---|
| ISBE 1915 | CrossWire `ISBE` 2.2: PD | CrossWire | — | Use SWORD (Free to share) |
| Nave's Topical Bible | CrossWire `Nave` 3.0: PD, **source CCEL** | — | — | **Excluded** (CCEL). Needs an independent source, e.g. Internet Archive scans |
| Torrey's Topical Textbook | CrossWire `Torrey` 1.3: PD (source bf.org) | CrossWire | — | Use SWORD |
| Hitchcock's Bible Names | CrossWire `Hitchcock`: PD | — | — | Already in the Starter library |
| Barnes' Notes | CrossWire `Barnes` 1.1: PD (NT only) | CrossWire | — | Use SWORD |
| Adam Clarke | CrossWire `Clarke` 2.0: PD; source Wikisource ("Clarke is missing from the resources proposed by CCEL") | CrossWire | — | Use SWORD |
| John Gill | No CrossWire module. Xiphos `Gill` 1.4 has no `DistributionLicense` (Unclear), and its About says "All Rights Reserved, Larry Pierce" | — | — | **Not free.** Needs another digitization |
| Keil & Delitzsch (English) | CrossWire `KD` 2.0: PD; source Wikisource | CrossWire | — | Use SWORD |
| Treasury of David | CrossWire `TDavid` 2.1: PD; source archive.spurgeon.org | CrossWire | — | Use SWORD |
| Vincent's Word Studies | PD by age (1887–1900). No module in any repository on the master list | Internet Archive `wordstudiesinnew0000marv` (1887) and later volumes | OCR project | **Defer** |
| Calvin (1840s translation) | CrossWire `CalvinCommentaries`: PD, **source CCEL** | — | — | **Excluded** (CCEL). Needs an independent source |
| **Robertson's Word Pictures** (US status) | CrossWire `RWP` 2.0: "Copyrighted; Free non-commercial distribution". Its About says vols 1–4 are PD, and "Volume 5 (c) 1932. Renewal 1960 Broadman Press". A renewed 1932 US copyright runs 95 years, to the end of 2027; vol. 6 (1933) runs to the end of 2028 | CrossWire | — | **Not free to share in the US until 2029-01-01** (vols 5–6). Personal use through SWORD meanwhile |
| Treasury of Scripture Knowledge | CrossWire `TSK` 1.4: PD (a Commentary module) | CrossWire | A Cross-reference set converter from the module | **Later.** The Starter's OpenBible.info set is TSK-derived already |
| OpenBible.info geocoding | CC BY 4.0 (GitHub licence `CC-BY-4.0` for `openbibleinfo/Bible-Geocoding-Data`). Parts derived from OpenStreetMap are ODbL (see [open-content-inventory.md](open-content-inventory.md)) | <https://github.com/openbibleinfo/Bible-Geocoding-Data> | A People & places geodata importer | **Later**, with a maps feature. Keep ODbL data separate |

## 6. What the Open catalogue lists now

Each entry downloads from eBible.org and is checked by the `copr.htm` in its zip. Each has an
evidence file quoting that notice and an Internet Archive capture of the publisher's licence
page. All 11 downloaded, passed the check and imported on 2026-11-06
(`live_every_open_catalogue_entry_downloads_checks_and_imports`):

| id | Title | Lang | Licence | Books | sha256 seen (not pinned) |
|---|---|---|---|---:|---|
| `ylt` | Young's Literal Translation | en | PD | 66 | `c526b4e3…` |
| `darby` | Darby Translation | en | PD | 66 | `ebdd506c…` |
| `geneva-1599` | Geneva Bible 1599 | en | PD | 66 | `6ed7356e…` |
| `webster` | Noah Webster Bible | en | PD | 66 | `0f3c1122…` |
| `oeb-us` | Open English Bible (U.S. spelling) | en | CC0 | 44 | `da884dcf…` |
| `lsv` | Literal Standard Version | en | CC BY-SA 4.0 | 66 | `b5fd551d…` |
| `douay-rheims` | Douay-Rheims 1899 | en | PD | 73 | `fcb5de45…` |
| `luther-1912` | Lutherbibel 1912 | de | PD | 66 | `76fb83f1…` |
| `segond-1910` | Louis Segond 1910 | fr | PD | 66 | `64c4fe2a…` |
| `reina-valera-1909` | Santa Biblia — Reina Valera 1909 | es | PD | 66 | `56937c2b…` |
| `vulgate-clementine` | Clementine Vulgate 1598 | la | PD | 73 | `5d73ce34…` |

**Why eBible entries are not pinned.** Every file in each zip is dated 2026-10-30, even
`engylt`, whose text dates from 2019. eBible rebuilds every zip on each run, so the sha256
drifts while the text does not. The ticket asks for downloads from the publisher's own URL
(not an Internet Archive copy, as the Starter library uses), so the pin is replaced by a check
that the licence notice inside the download still says what the evidence quotes.
