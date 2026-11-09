# Language names in Get Resources: live catalog check (issue #244)

**Question:** Which language tags do the SWORD repositories' catalogs use, can every one be named from Unicode CLDR and the ISO 639-3 code tables, and how many Bengali Bibles does the live catalog offer?
**Date:** 2026-11-08, from a Linux host against the live repositories while building #244.
**How:** `curl` of `https://crosswire.org/ftpmirror/pub/sword/masterRepoList.conf`, then each repository's `mods.d.tar.gz` (HTTPS where the master list gives one, FTP for IBT and STEP Bible), and `grep` of every conf's `Lang=` line. The tags are kept as the test list `crates/engine/tests/assets/sword-catalog-languages.txt`, and `every_language_tag_of_the_live_sword_catalog_has_a_name` checks each one has a name. Then the ignored live test `live_searching_bengali_bangla_or_ben_finds_the_live_catalogs_bengali_bibles` (`apps/openbible/src/live_tests.rs`) fetched the catalog through the app's own transport and searched the Bibles' language filter.

## Repositories and tags

All 11 repositories on the master list answered: CrossWire (428 confs), CrossWire Beta (11), Bible.org (5), Xiphos (84), CrossWire Attic (88), IBT (155), CrossWire Wycliffe (44), eBible.org (1530), Deutsche Bibelgesellschaft (1), Lockman Foundation (4) and STEP Bible (61). Four confs have no `Lang=`; SWORD reads those as `und`.

Their confs use 1384 different `Lang=` values:

- Mostly ISO 639-1 (`en`, `bn`) or ISO 639-3 (`kmo`, `hbo`) codes alone. eBible.org's are all ISO 639-3 codes where there is no 639-1 code.
- Language and script: `zh-Hant`, `sr-Latn`, `uz-Cyrl`, many Cyrillic-script languages of Russia on IBT (`tyv-Cyrl`, `cjs-Cyrl`).
- Language, script and region, all on STEP Bible: `ben-Beng-IN`, `pl-Latn-PL`, `zh-Hant-HK`, `vi-Zinh-VN`.
- One language under more than one tag: `en` and `en-Latn`, `os` and `oss-Cyrl`, `kk` and `kk-Cyrl`, `kbd` and `kbd-Cyrl`, `krc` and `krc-Cyrl`, `lez` and `lez-Cyrl`, `tkr` and `tkr-Latn`.
- SWORD's own dialect tags: `cop-sa` (Sahidic Coptic), which BCP 47 would read as Coptic of Saudi Arabia.
- Retired ISO 639-3 codes: eBible.org tags a Bible `daf` (Dan), split in 2013 into `dnj` and `lda`.

## Naming them

| Data | What it names | Licence |
|---|---|---|
| Unicode CLDR 49 `cldr-localenames-full` (npm, sha256 `d00bafb5…`) | English names of 704 language and locale ids, of scripts and of regions; each language's name in its own locale ("বাংলা") | Unicode License v3 |
| Unicode CLDR 49 `cldr-core` (npm, sha256 `085b1c7d…`), `likelySubtags.json` | the script each language is usually written in (`bn` → `Beng`), so `ben-Beng-IN` reads as Bangla (India) | Unicode License v3 |
| ISO 639-3 code tables `iso-639-3_Code_Tables_20261015.zip` (SIL, sha256 `1b75148f…`) | the reference name of every code CLDR lacks (`kmo` "Kwoma"); which 639-1, 639-2/B and 639-3 codes are one language; retired codes | SIL's terms of use, below |

With both, all 1384 tags get a name; none shows as "Unknown language". CLDR alone names the language of only 256 of them: most of eBible.org's ISO 639-3 codes are languages CLDR has no name for.

CLDR's English name for `bn` is "Bangla"; ISO 639-3's reference name is "Bengali". The app shows "Bangla — বাংলা" and keeps "Bengali" as another name a search finds, so "Bengali", "Bangla", "বাংলা", "ben" and "bn" all find it.

## Bengali Bibles in the live catalog

The live catalog offers **4 Bengali Bibles**, under two language filter choices. The filter lists 1375 languages for Bibles, every one named:

| Choice | Module | Repository | `Lang=` | Licence in conf |
|---|---|---|---|---|
| Bangla — বাংলা · 3 | `ben2006eb` (পবিত্র বাইবেল) | eBible.org | `bn` | Copyrighted; Permission to distribute granted to eBible.org |
| | `benirv2019eb` (Indian Revised Version, Bengali) | eBible.org | `bn` | Creative Commons: by-sa |
| | `benobcv2019eb` (Biblica Bengali Contemporary Version) | eBible.org | `bn` | Creative Commons: by-sa |
| Bangla (India) — বাংলা · 1 | `Ben_BSS` (Bengali Contemporary Version 2019, "Bangladesh, India") | STEP Bible | `ben-Beng-IN` | Creative Commons: BY-SA 4.0 |

"Bengali", "Bangla" and "ben" each find both choices, Bangla first. "ben" also finds Benabena (`bef`) and Boko (Benin) (`bqc`), whose names hold it, after the languages whose code it is.

## Licence notes

- **CLDR**: Unicode License v3, which allows redistribution with the licence text. `crates/engine/assets/languages/LICENSE-CLDR` is the package's LICENSE, unmodified, and Credits shows it.
- **ISO 639-3**: the terms on <https://iso639-3.sil.org/code_tables/download_tables> (checked 2026-11-08) allow the code set to be "downloaded and incorporated into software products … either commercial or non-commercial" if attribution is given to iso639-3.sil.org, the identifiers are not modified or extended, and the product does not provide a means to redistribute the code set. The app keeps the codes and reference names unchanged, credits iso639-3.sil.org in Credits and `ATTRIBUTION.toml`, and offers no way to export the table. The terms also say the ISO 639-3 website is the only authorized distribution site for the code tables. We read the built-in table as the code set incorporated into the software, not as a redistribution of the code tables; `cargo xtask language-names` fetches the tables from iso639-3.sil.org itself.
