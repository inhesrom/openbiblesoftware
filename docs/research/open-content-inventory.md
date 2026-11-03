# Open Content Inventory — v0.1 Starter Library (issue #2)

**Question:** Which public-domain or openly licensed (CC0 / CC BY / CC BY-SA) resources can ship in a v0.1 starter library for a serious lay student, and what are their license obligations, sources, formats, and quality caveats?
**Date:** 2026-11 (research pass; sources accessed this session)
**Scope:** content only (not code). Project code is MIT/Apache-2.0; content gets its own per-resource license notices.

## TL;DR

- **Bibles, no strings:** KJV, ASV, WEB, YLT, Darby are marked public domain on eBible.org, and eBible offers USFM/USFX/VPL downloads. BSB was dedicated **CC0** on 2023-04-30. Name rules apply: changed WEB text can't be called "World English Bible" (trademark), and changed BSB text shouldn't use the "Berean" name.
- **KJV caveat:** eBible says the Crown letters patent apply only to *printing* in or importing into the UK. Outside the UK the text is "firmly in the Public Domain". A digital app is not printing, but we have not had a lawyer confirm this for UK distribution.
- **LSV is CC BY-SA 4.0.** We can ship it next to MIT/Apache code (aggregation), but the LSV file stays BY-SA, needs full attribution ("Literal Standard Version (LSV)" + Covenant Press), and any edits must stay BY-SA.
- **Strong's-tagged KJV:** use **eBible `eng-kjv2006`** ("with Strong's numbers added", PD). The CrossWire `KJV` module is listed with **Distribution License: GPL**, even though its About text grants use "for any purpose". That's ambiguous, so avoid it.
- **STEPBible-Data (TAHOT, TAGNT, TBESH, TBESG, TFLSJ, TIPNR, TVTMS, TEHMC/TEGMC) is CC BY 4.0.** Credit "STEP Bible" with a link to STEPBible.org, and record any changes we make. TSV format. This is the best open source for original-language tagging, lexicons, proper names, and versification. Skip TTESV, because the ESV text is not open.
- **Lexicons:** Strong's Hebrew and Greek are PD (openscriptures/strongs XML). The OSHB BDB plus Strong's lexicon is **CC BY 4.0** (markup; the underlying text is PD). Dodson is PD/CC0. Abbott-Smith is PD. For **Thayer** we found no clean primary-source license, so treat it as unverified.
- **Cross-references:** OpenBible.info's ~340k cross-refs are **CC BY 4.0** (TSK-derived, 2 MB zip). The CrossWire TSK module is PD.
- **Commentaries and dictionaries** (MHC, JFB, Barnes, Clarke, Easton, Smith, ISBE, Hitchcock) are listed as PD on CrossWire. **Don't take files from CCEL**: CCEL limits use to "personal, educational, or non-profit purposes" and asks for permission to republish.
- **Maps:** OpenBible Bible-Geocoding-Data is **CC BY 4.0**, but parts derived from OpenStreetMap are **ODbL 1.0** (share-alike for databases). Photos and thumbnails each have their own licenses.
- **Licensing pattern:** every resource carries its own license and attribution. Keep a `CONTENT-LICENSES` manifest and an in-app "About this resource" panel. Keep BY-SA and ODbL content in separate files so share-alike stays limited to them.

## 1. English Bibles

| Resource | License (primary source) | Obligations | Canonical download / formats | Notes |
|---|---|---|---|---|
| **KJV 1769 + Apocrypha** (`eng-kjv`) | Public Domain; Crown letters patent restrict printing in or importing *printed copies* into the UK only ([eBible](https://ebible.org/find/show.php?id=eng-kjv)). CUP: "Rights in the Authorized (King James) Version… administered in the United Kingdom by the Crown's patentee, Cambridge University Press" ([CUP copyright page](https://assets.cambridge.org/97805211/63347/copyright/9780521163347_copyright_info.pdf)) | None outside the UK. UK digital distribution: **unverified**, legal question | eBible: HTML, VPL, USFM, USFX, OSIS-SWORD, ePub, PDF ([eBible](https://ebible.org/find/show.php?id=eng-kjv)) | Standardized 1769 text with Apocrypha |
| **KJV + Strong's** (`eng-kjv2006`) | Public Domain, same patent note ([eBible](https://ebible.org/find/show.php?id=eng-kjv2006)) | Same as above | Same formats: [USFM zip](https://eBible.org/Scriptures/eng-kjv2006_usfm.zip) | "protocanon only, with Strong's numbers added". Strong's markup is inside the USFM `\w` attributes (*inferred from USFM convention; check the file*) |
| CrossWire `KJV` module (Strong's + morphology) | About text: copyright CrossWire 2003-2023, "grants a general public license to use this text for any purpose". **Distribution License field: GPL** ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=KJV)) | Unclear: GPL label vs. "any purpose" grant | SWORD module, 4.03 MB installed | Better NT tagging (Robinson TR, 2016 data), but the license is ambiguous. Get written clarification from CrossWire before using |
| **ASV 1901** | Public domain ([eBible](https://ebible.org/find/show.php?id=eng-asv)) | None | eBible: USFM/USFX/VPL/SWORD/HTML | — |
| **WEB** (Classic) | Public domain; "World English Bible" is a **trademark** of eBible.org. If you change the text, don't call it WEB ([eBible](https://ebible.org/find/show.php?id=eng-web)) | Name restriction only | eBible: USFM/USFX/VPL/SWORD/HTML/MP3 | 2020 "stable text", includes Apocrypha, uses "Yahweh" ([eBible](https://ebible.org/find/show.php?id=eng-web)) |
| **BSB** (Berean Standard Bible) | **CC0** dedication as of 2023-04-30; "All uses are freely permitted". Attribution is appreciated but not required ([berean.bible terms](https://berean.bible/terms.htm)) | Courtesy request: if the text differs from the official text, don't use the "Berean" name | [berean.bible/downloads](http://berean.bible/downloads.htm): TXT, USFM, USX, USJ, xlsx, ePub. Also `bsb_tables.tsv` (word-level Hebrew/Greek alignment) | Modern, readable; good default reading Bible. The same page also lists BLB NT, BRB, and the Majority Bible (CC0 per terms) |
| BSB translation tables | The terms dedicate the "Berean Bible… texts" to CC0 ([terms](https://berean.bible/terms.htm)). A third-party review says rights are "mixed by column" ([atlasdataset](https://atlasdataset.com/research?id=berean-translation-tables), **secondary, unverified**) | Check before shipping | `bsb_tables.tsv` ([downloads](http://berean.bible/downloads.htm)); 23 columns per [BSB-publishing/usj2bsb](https://github.com/BSB-publishing/usj2bsb) | Could give a reverse interlinear for BSB with Strong's. Needs a license check |
| **YLT** | Public Domain (Robert Young) ([eBible](https://ebible.org/find/show.php?id=engylt)) | None | eBible: USFM etc. | Very literal, archaic |
| **Darby** | Public Domain (J. N. Darby) ([eBible](https://ebible.org/find/show.php?id=engDBY)) | None | eBible: USFM etc. | — |
| **LSV** | Copyright © 2020 Covenant Press, released under **CC BY-SA 4.0** ([Covenant Press](https://www.covenantpress.org/p/literal-standard-version.html); [eBible copyright page](https://ebible.org/englsv/copyright.htm)) | Full attribution when distributing the whole Bible: "Literal Standard Version (LSV)" + Covenant Press. Mark any changes. Redistributed contributions must stay BY-SA. Publisher asks that the text stay unaltered | eBible USFM (the `eng-lsv` ID lookup failed this session; copyright page is at `/englsv/`) | Some sources mention "non-commercial" wording ([freely-given.org](https://freely-given.org/BibleTranslations/English/LSV/), secondary). The primary pages say plain CC BY-SA |

## 2. Original-language tagged texts and lexicons

| Resource | License | Obligations | Source / format | Notes |
|---|---|---|---|---|
| **STEPBible TAHOT** (Hebrew OT) | **CC BY 4.0** ([STEPBible-Data README](https://github.com/STEPBible/STEPBible-Data)) | Credit "STEP Bible" with a link to www.STEPBible.org. Record changes and make them available. Mirrors must stay current and link back ([README](https://github.com/STEPBible/STEPBible-Data)) | Tab-separated UTF-8, split into 4 files (Gen-Deu, Jos-Est, Job-Sng, Isa-Mal) in `Translators Amalgamated OT+NT/` ([repo tree](https://github.com/STEPBible/STEPBible-Data/tree/master/Translators%20Amalgamated%20OT%2BNT)) | Leningrad via OSHB/Westminster, corrected from scans. Full morphology plus disambiguated Strong's tags that still work with plain Strong's |
| **STEPBible TAGNT** (Greek NT) | CC BY 4.0 (same) | Same | TSV, 2 files (Mat-Jhn, Act-Rev) | Includes every word in NA27/28, TR, SBLGNT, Byz, WH, THGNT, marked by edition. Robinson-style morphology plus context-sensitive glosses. Gives KJV (TR) and modern-edition support from one file |
| **TBESH** (Hebrew lexicon) | CC BY 4.0 | Same | TSV in `Lexicons/` | "Abridged BDB linked to extended Strongs" ([README](https://github.com/STEPBible/STEPBible-Data)) |
| **TBESG** (Greek lexicon) | CC BY 4.0 | Same | TSV | Brief definitions from corrected Abbott-Smith; covers NT, LXX, and variants |
| TFLSJ (full LSJ for Bible words) | CC BY 4.0 | Same | TSV | Optional; scholarly depth |
| TIPNR (proper names) | CC BY 4.0 | Same | TSV, multi-line records | Every proper noun, with refs, family links, and geolocation (based on OpenBible). **Descriptions were "created by Claude 3 AI"** ([README](https://github.com/STEPBible/STEPBible-Data)). Label as AI-generated or leave them out |
| TVTMS (versification map) | CC BY 4.0 | Same | TSV | Infrastructure for lining up KJV, Hebrew, LXX, and Latin verse numbering. Needed for parallel view |
| TTESV | Tags only. **The ESV text is not open** | — | — | Not usable without an ESV license |
| **Strong's Hebrew & Greek dictionaries** (openscriptures/strongs) | Source text 1890/1894 = PD. Greek XML prologue: "Public Domain -- Copy Freely" (U. Petersen). Hebrew OSIS header: `<rights>Public Domain</rights>` (cloned repo files `greek/StrongsGreekDictionaryXML_1.4/strongsgreek.xml`, `hebrew/StrongHebrewG.xml` in [openscriptures/strongs](https://github.com/openscriptures/strongs)) | None. No top-level LICENSE file was seen in the repo | XML, JS/JSON | Cleaned and corrected editions |
| **OSHB Hebrew Lexicon** (BDB + Strong's + LexicalIndex) | **CC BY 4.0** for the files; the BDB and Strong's text itself stays PD ([HebrewLexicon README](https://github.com/openscriptures/HebrewLexicon)) | Credit "the Open Scriptures Hebrew Bible Project" | XML + XSD | The BDB part is "a work in progress" (incomplete refs) |
| BDB Glosses (SWORD) | Listed PD; based on the OSHB lexicon ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=BDBGlosses_Strongs)) | Treat as CC BY (OSHB attribution) to be safe | SWORD | — |
| **Dodson Greek Lexicon** | PD; repo license CC0 ([biblicalhumanities/Dodson-Greek-Lexicon](https://github.com/biblicalhumanities/Dodson-Greek-Lexicon); [README-ulrikp](https://github.com/biblicalhumanities/Dodson-Greek-Lexicon/blob/master/README-ulrikp.txt)) | None | CSV/XML | Short glosses keyed to Strong's and GK numbers |
| Abbott-Smith (1922) | "The lexicon (abbott-smith.tei.xml)… is in the public domain" ([project page](http://translatable-exegetical-tools.github.io/Abbott-Smith/)) | None | TEI XML | The repo's working file contains uncleaned OCR. Use release builds |
| **Thayer** (1889) | Text published 1889 (PD in US by age). **No authoritative digital-edition license found.** A 2006 CrossWire list thread asks why Thayer was pulled and whether it was "not actually in the public domain" ([sword-devel](http://crosswire.org/pipermail/sword-devel/2006-March/023597.html)); unresolved | **Unverified** | Third-party GitHub datasets exist ([example](https://github.com/EveryPromiseBible/Every-Promise-Thayers)) but are not primary | Defer, or create our own digitization from page scans |

## 3. Cross-references

| Resource | License | Obligations | Source / format | Notes |
|---|---|---|---|---|
| **OpenBible.info cross-references** | **CC BY 4.0** ("Unless otherwise indicated…") ([OpenBible](https://www.openbible.info/labs/cross-references/)) | Credit OpenBible.info | [cross-references.zip](https://a.openbible.info/data/cross-references.zip), **2 MB**. Contents are a TSV of from/to/votes (*commonly reported; check the file*) | About 340,000 refs, "primarily" from TSK, with votes seeded from the topical/Twitter data. Votes allow relevance ranking |
| **TSK** (Treasury of Scripture Knowledge) | Public Domain ([CrossWire TSK](https://crosswire.org/sword/modules/ModInfo.jsp?modName=TSK)) | None | SWORD commentary module, 2.73 MB installed | About 500k refs (Canne, Browne, Blayney, Scott, c. 1880). Keyed to KJV phrases. Usable directly (PD) |

## 4. Commentaries

| Resource | License (CrossWire listing) | Size (SWORD install) | Notes |
|---|---|---|---|
| Matthew Henry Complete (MHC) | PD, "Copy Freely" ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=MHC)) | 14.88 MB | Text "prepared from the Christian Classics Ethereal Library". Text is PD; see the CCEL note below |
| Jamieson-Fausset-Brown (1871) | PD ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=JFB)) | 5.74 MB | Whole Bible |
| Barnes' NT Notes | PD ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=Barnes)) | 5.82 MB | NT only in this module |
| Adam Clarke | PD ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=Clarke)) | 8.46 MB | Whole Bible |
| John Gill | Author died 1771, so the text is PD by age. **CrossWire module page failed to load this session**, and an old CrossWire thread mentions a typist claiming copyright ([sword-devel 2000](http://crosswire.org/pipermail/sword-devel/2000-July/010213.html)) | ? | **Unverified** digital provenance |

**Starter sources for MHC and JFB:** chosen in [commentary-sources.md](commentary-sources.md) (#39): the Wikisource MHC transcription and the Woodside / OnLine Bible JFB edition from thirdmill.org, both pinned through Internet Archive captures.

**CCEL caveat:** CCEL says most editions are based on US-PD books, but "These books may be used for personal, educational, or non-profit purposes. Contact us for permission to republish CCEL works or to use them commercially" ([CCEL copyright](https://www.ccel.org/about/copyright.html)). CrossWire developers have described this as a claim on the ThML/XML markup, not the text ([sword-devel 2010](http://crosswire.org/pipermail/sword-devel/2010-November/035428.html)). Two options: get files from CrossWire modules or our own scans and keep no CCEL markup, or ask CCEL for permission.

**CrossWire software note:** the SWORD *software* is GPL, but "texts distributed with the software need not be released under any particular license". Each module's About section sets its terms ([CrossWire license](https://www.crosswire.org/sword/about/license.jsp)). Reading SWORD module files with our own pure-Rust importer does not link libsword.

## 5. Dictionaries / encyclopedias

| Resource | License | Size | Notes |
|---|---|---|---|
| Easton's Bible Dictionary (1897) | PD, "Copy Freely" ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=Easton)) | ~MB (*unverified*) | Illustrations not included |
| Smith's Bible Dictionary (1884) | PD ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=Smith); [smith.conf](https://www.crosswire.org/svn/sword-tools/trunk/modules/conf/release/mods.d/smith.conf)) | 3.33 MB | ThML source |
| ISBE (1915 ed., Orr) | PD ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=ISBE)) | large (*unverified*) | Most in-depth of the free encyclopedias |
| Hitchcock's Bible Names | PD, "feel free to copy and distribute" ([CrossWire](https://www.crosswire.org/sword/modules/ModInfo.jsp?modName=Hitchcock)) | small | Over 2,500 names with meanings |

## 6. Maps / places

| Resource | License | Obligations | Source / format | Notes |
|---|---|---|---|---|
| **OpenBible Bible-Geocoding-Data** (2021+) | **CC BY 4.0**. OSM-derived data is **ODbL 1.0**. Images vary ([repo README](https://github.com/openbibleinfo/Bible-Geocoding-Data); [OpenBible geo](https://www.openbible.info/geo/)) | Credit OpenBible.info. ODbL share-alike applies to derived databases from OSM parts. Check each image's license | JSONL (`ancient`, `modern`, `geometry`, `image`, `source`) plus GeoJSON/KML. Thumbnails zip is **180 MB** | Confidence scores for each identification; Wikidata links; ten English translations' place-name forms |
| Older 2007 geocoding | Same site license (CC BY) | Credit | `places.txt`, `merged.txt` TSV ([OpenBible geo](https://www.openbible.info/geo/)) | Simpler, smaller fallback |

Basemap tiles and imagery are **not** covered here. Offline basemaps need a separate decision (OSM data is ODbL; Natural Earth is PD, *not researched here*).

## 7. Compatibility with MIT/Apache-2.0 code

- **PD/CC0** (KJV outside the UK, ASV, WEB, YLT, Darby, BSB, Strong's, Dodson, Abbott-Smith, TSK, CrossWire-listed commentaries and dictionaries): no conflict.
- **CC BY 4.0** (STEPBible, OpenBible cross-refs/geo, OSHB lexicon): compatible. Requires attribution and a license link, plus STEPBible's request to record changes ([README](https://github.com/STEPBible/STEPBible-Data); [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)).
- **CC BY-SA 4.0** (LSV) and **ODbL** (OSM parts): share-alike covers adaptations of *that* material, not separate works bundled with it ([CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)). Shipping them as separate resource files next to MIT/Apache code is aggregation. *Our reading of the license, not legal advice.* Risk: if our converter merges LSV into a combined database with other texts, the combined artifact could be argued to be an adaptation. Keep one file per resource.
- **GPL-labelled data** (CrossWire KJV module): avoid until clarified. A GPL label on data is murky, and the locked decision excludes GPL.
- **Non-commercial or permission-required** (CCEL markup, ESV/TTESV): exclude.

## 8. Suggested v0.1 bundle (options, with sizes)

| Tier | Contents | Rough size |
|---|---|---|
| Minimal | BSB, KJV+Strong's (eng-kjv2006), WEB, ASV; Strong's dictionaries; OpenBible cross-refs; Easton | ~25-40 MB raw text (*estimate*) |
| Study | + YLT, Darby; STEPBible TAHOT/TAGNT/TBESH/TBESG/TVTMS; Dodson; MHC, JFB; Smith, Hitchcock; OpenBible geocoding (no thumbnails) | ~100-200 MB raw (*estimate*; STEPBible TSVs are large) |
| Optional download | LSV (BY-SA), Clarke, Barnes, ISBE, TSK, TFLSJ, geocoding thumbnails (180 MB) | extra |

## Implications for our decisions

1. **Resource format must store license metadata** for each resource: license ID, attribution string, source URL and version, a "modified?" flag, and a change log (needed for STEPBible and BY-SA). The UI needs an attribution panel.
2. **Versification** (TVTMS) is a core engine concern if KJV, BSB, WEB, Apocrypha, and Hebrew/Greek are shown side by side.
3. **Importer priorities:** USFM/USX (eBible, BSB), STEPBible TSV, OpenBible TSV/JSONL, OSIS/TEI/ThML XML (lexicons, dictionaries). A pure-Rust SWORD module reader is optional and only needed for the PD commentaries and dictionaries.
4. **Name and trademark rules:** if we normalize or correct WEB or BSB text, we must rename it or keep the text verbatim.
5. **Get legal or permission sign-off on:** KJV in UK digital distribution; CrossWire KJV GPL label; BSB tables column rights; Thayer and Gill digital provenance; CCEL.
6. **AI-generated content:** STEPBible TIPNR descriptions are AI-written. Decide on a policy (label, exclude, or include).

## Gaps / unverified

- Thayer license and provenance, and Gill module status (page didn't load).
- Exact column layout inside the OpenBible cross-ref zip; whether Strong's markup is in eBible USFM; exact sizes of STEPBible files, Easton, and ISBE.
- LSV "non-commercial" confusion (only secondary sources mention it).
- KJV patent as it applies to UK *digital* distribution. No primary legal source found.
- BSB `bsb_tables.tsv` column-level rights (one secondary source).
