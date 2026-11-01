# Open Bible Format Survey (issue #3)

**Question:** Which open Bible/resource formats should v0.1 import, and how feasible is a pure-Rust importer (no libsword, no GPL linking) for each?
**Date:** 2026-11 (source timestamps observed up to 2026-11-01). Research only. This file does not make the decision.

## TL;DR

- **USFM 3.x / USX** has the best specification: openly maintained, with RNC/RNG schemas, now at `usfm-bible` / docs.usfm.bible. It covers notes (`\f`), cross-refs (`\x`), Strong's (`\w …|strong="G5485"`), words of Jesus (`\wj`) and headings. **MIT Rust crates already exist** (`usfm3`, `usfm`). Effort: low–medium.
- **OSIS 2.1.1** has the richest semantics for study texts (lemma/morph arrays, `q who="Jesus"`, notes, refs). Main gotcha: **verses can be containers or `sID`/`eID` milestones**, and XML validators cannot check whether milestones are paired correctly. A `quick-xml` (MIT) importer is a moderate job.
- **SWORD modules:** CrossWire says the binary format is **undocumented and "proprietary in the sense that we see no need to document it… We change it when we need to."** Even so, the **CC0 crate `sword-modules` 0.1.0** reads RawText/zText(4), RawCom/zCom(4), RawLD/zLD, RawGenBook, all compressions, cipher and 18 versifications, and documents a clean-room process. `rsword_chirho` is **GPL-2.0-or-later**, so we must avoid it.
- **SWORD has the largest freely downloadable catalog**: about 260 Bible modules on CrossWire's main page alone, plus commentaries and lexicons. **Licences vary per module** (`DistributionLicense` conf key). Entries are raw OSIS/ThML/GBF/TEI that must still be parsed, so SWORD import ⇒ also need OSIS + ThML + GBF markup parsers.
- **eBible.org** reports **1,295 freely redistributable translations in 1,024 languages**. Each is offered as USFM, USFX, VPL, SWORD, ePub, HTML and other formats. For English public-domain Bibles, USFM from eBible is enough for the starter library.
- **Zefania XML** is simple (BIBLEBOOK/CHAPTER/VERS). Its schema and docs are **GPL-licensed**. That should not cover a clean parser, but we should not vendor the XSD. Spec quality is mediocre and data quality uneven. Effort: low.
- **theWord Bibles are NOT SQLite.** They are plain text with exactly 31,102 lines (`.ont`) and custom tags. theWord *book* modules are SQLite. **MySword** modules are SQLite with GBF-like tags, and MySword itself says the schema is not fully documented. Both carry many copyrighted or encrypted modules. Effort: low for the format, but the licensing value is unclear.
- **SQLite formats affect WASM:** `rusqlite` bundles C SQLite. wasm32 support arrived only recently via `sqlite-wasm-rs` (PR #1769, merged 2025-12). That is not "pure Rust".
- **ePub:** `rbook` (Apache-2.0) reads EPUB 2/3. eBible ships ePub per translation. It is useful for general books, but it has no verse semantics unless we parse its HTML.
- Every importer must handle **versification mapping** (KJV vs. NRSV vs. Synodal, etc.). SWORD has 18 built-in systems. USX has `altnumber`. Paratext uses `.vrs` files.

## 1. Format-by-format

### 1.1 USFM 3 / USX 3 (USJ)

| Aspect | Facts |
|---|---|
| Spec | USFM 3.0 docs at ubsicap.github.io/usfm. Maintenance moved to github.com/usfm-bible, docs at docs.usfm.bible, with RNC/RNG grammar in `tcdocs` and a Python reference implementation `usfmtc` ([ubsicap/usfm README](https://github.com/ubsicap/usfm)). USFM 3.1 "standardize[s] a single content model" across backslash, XML (USX) and JSON (USJ) ([release notes](https://docs.usfm.bible/usfm/latest/release-notes.html)). The `tcdocs` licence shows as "Other" on GitHub ([tcdocs](https://github.com/usfm-bible/tcdocs)). **Exact terms are unverified.** |
| Structure | One file per book ([CrossWire File Formats](https://wiki.crosswire.org/File_Formats)). Uses `\c`/`\v` markers. `\w word\|strong="G5485" lemma=…` holds Strong's and lemma, with augmentations such as `G5485:a`. `\wj` marks words of Jesus ([USFM characters](https://ubsicap.github.io/usfm/characters/index.html)). There are markers for footnotes and cross-references (`\f`, `\x`), headings (`\s`) and poetry (`\q`) (the marker images are listed in the [repo](https://github.com/ubsicap/usfm)). USFM has no first-class morphology attribute (**unverified for 3.1**). |
| USX verses | In USX 3.0, `<verse sid>` … `<verse eid>` **milestones are required** at the start and end of each verse, and `vid` on `<para>` re-identifies the current verse ([USX 3.0 elements](https://ubsicap.github.io/usx/elements.html), [usx#34](https://github.com/ubsicap/usx/issues/34)). Verse numbers can be ranges such as `1-2`. `@altnumber` supports alternate versification ([USX elements](https://ubsicap.github.io/usx/v3.0.0/elements.html)). |
| Catalog | eBible.org has 1,295 freely redistributable translations and 1,024 languages ([scorecard](https://ebible.org/Scriptures/scorecard.txt)). The WEB (public domain) is offered as USFM, USFX, VPL, SWORD, ePub, HTML, PDF and more ([WEBP details](https://ebible.org/find/details.php?id=engwebp)). KJV, ASV and WEB USFM downloads are linked from [CrossWire File Formats](https://wiki.crosswire.org/File_Formats). |
| Rust crates | **`usfm3`** 0.3.1 (MIT): error-tolerant, parses USFM 3.x into USJ/USX/CST, has diagnostics and verse-text extraction, ships npm/WASM bindings, and was created 2026-02 ([docs.rs](https://docs.rs/crate/usfm3/latest), [crates.io](https://crates.io/crates/usfm3)). **`usfm`** 0.2.10 (MIT, pest-based, ~2.2k downloads) ([crates.io](https://crates.io/crates/usfm)). **`usfm-grammar`** 3.0.0-beta.1 (MIT, tree-sitter based, so it has a C dependency, which hurts WASM purity) ([crates.io](https://crates.io/crates/usfm-grammar)). |
| Effort | **Low–medium** if we use `usfm3` and map its tape or USJ into our model. Medium if we write our own. |
| Gotchas | Peripheral books. Verse ranges and lettered verses (`[0-9]+\w?`). Non-default versification. Publisher-specific deviations, which is why `usfm3` is "error-tolerant". |

### 1.2 OSIS 2.1.1

| Aspect | Facts |
|---|---|
| Spec | An XML Schema. The Bible Technologies Group no longer exists, so the schema and docs now live at ebible.org/osis and on CrossWire, which uses the `osisCore.2.1.1-cw-latest.xsd` variant ([OSIS Bibles](https://wiki.crosswire.org/OSIS_Bibles)). The OSIS 2.1.1 User Manual PDF is hosted at [crosswire.org/osis](https://crosswire.org/osis/OSIS%202.1.1%20User%20Manual%2006March2006.pdf). The manual is good but dated (2006). |
| Structure | `<div type="book">`, `<chapter>`, `<verse osisID>`. Strong's and morphology use `<w lemma="strong:G3588 strong:G80" morph="robinson:T-APM robinson:N-APM" src="7 8">`, where lemma/morph/src are **parallel arrays**. Words of Christ use `<q who="Jesus">`. `<transChange type="added">` marks italics. Also has `x-split` words, glosses and `xlit` ([OSIS Bibles](https://wiki.crosswire.org/OSIS_Bibles)). OSIS also covers notes, references, titles and commentaries ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)). |
| **Milestone gotcha** | Document structure (book/section/paragraph) and verse structure (book/chapter/verse) can overlap. Verses and chapters may be containers or `sID`/`eID` milestones. **There is no milestoned `<p>`**, so a paragraph that spans verses forces milestoned verses. A validator cannot check consistent milestoning, sID/eID pairing or non-overlap. `!` sub-identifiers in osisIDs exist (osis2mod strips them). Pretty-printing breaks significant whitespace ([OSIS Bibles](https://wiki.crosswire.org/OSIS_Bibles)). |
| Catalog | CrossWire's preferred source format for Bibles and commentaries ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)). Standalone OSIS downloads are less common than USFM. The eBible WEBP page lists no OSIS download ([WEBP](https://ebible.org/find/details.php?id=engwebp)). Most OSIS reaches users inside SWORD modules. |
| Rust crates | No dedicated permissive and mature OSIS crate was verified. `rsword_chirho::import_chirho::OsisParserChirho` is **GPL-2.0+** ([repo](https://github.com/loveJesus/rsword-chirho)). `logoxide-text` 0.1.2 reads OSIS, USX/USFM (via usfm3), USJ, ThML and GBF, and is WASM-capable ([docs.rs](https://docs.rs/crate/logoxide-text/latest)). The logoxide project is CC0-1.0 on [PyPI](https://pypi.org/project/logoxide/). **The licence of the `logoxide-text` crate itself is unverified.** The base parser would be `quick-xml` (MIT) ([crates.io](https://crates.io/crates/quick-xml)). |
| Effort | **Medium.** It needs a streaming parser with a state machine that tracks the current verse across milestones and attaches notes, `<w>` and `<q>`. |

### 1.3 SWORD modules (conf + RawText/zText/zText4, RawCom/zCom, RawLD/zLD, GenBook)

| Aspect | Facts |
|---|---|
| Spec | **No official spec.** "Other than the source code for the SWORD API, there is no documentation for the file format… proprietary in the sense that we see no need to document it… We change it when we need to." CrossWire recommends using the API ([File Formats](https://wiki.crosswire.org/File_Formats)). The conf-file keys *are* documented ([DevTools:conf Files](https://wiki.crosswire.org/DevTools:conf_Files)). Versification: a conf entry selects the system, KJV is the default, and the systems are hard-coded canon headers ([Alternate Versification](https://wiki.crosswire.org/Alternate_Versification)). |
| Layout (per `sword-modules` docs) | A `mods.d/*.conf` file in INI style, with repeatable keys and an RTF-ish `About`. Defaults: SourceType=GBF, Encoding=Latin-1, CompressType=LZSS, BlockType=CHAPTER, Versification=KJV. Verse-keyed modules keep an index slot for *every* position in the versification (module heading, testament heading, book intro, chapter intro, then verses). Raw modules use `ot.vss` (`u32 offset, u16 size`; `u32` in the "4" variants). Compressed modules chain `.Xzv` → `.Xzs` → `.Xzz` blocks, with X = b/c/v. LD modules use `.idx`/`.dat` records containing `KEY\r\n` and `@LINK`. Little-endian throughout. Compression: LZSS, ZIP, BZIP2 or XZ. Optional Sapphire II cipher ([sword-modules](https://docs.rs/crate/sword-modules/latest)). `mod2zmod` block types are book/chapter/verse, and compression is LZSS or Zip ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)). |
| Structure | Entries hold raw OSIS, ThML, GBF, TEI or plain text according to `SourceType`. Per-verse markup is **not well-formed XML**, because quotes and paragraphs span verses as milestones or split tags ([sword-modules](https://docs.rs/crate/sword-modules/latest)). Notes, cross-refs, Strong's, morphology and red letter come through the embedded markup (OSIS examples in [OSIS Bibles](https://wiki.crosswire.org/OSIS_Bibles)). |
| Catalog | CrossWire Bibles page lists about 260 Bible modules, counted by download links on 2026-11 ([ModDisp](https://www.crosswire.org/sword/modules/ModDisp.jsp?modType=Bibles)). There are also commentaries, lexicons (e.g. StrongsGreek) and genbooks, plus affiliated repos (eBible.org/sword, IBT, STEP) ([sword-modules](https://docs.rs/crate/sword-modules/latest)). **The licence is per module**: submission requires `DistributionLicense` and `TextSource` ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)), and some modules are locked or commercial. |
| Rust crates | **`sword-modules` 0.1.0 — CC0-1.0.** A read-only reader that is WASM-capable through its `Files` trait. Tested byte-identical to libsword `mod2imp` across 15 real modules (~207k entries). It does *not* parse markup, and does not do cross-versification mapping, search indexes, HREFCom or writing. It states that it contains no libsword code and was reimplemented from behavioural specs, documented in `CLEANROOM.md` and `docs/spec/`. Its installer is optional, behind a cargo feature ([docs.rs](https://docs.rs/crate/sword-modules/latest)). **`rsword_chirho` 0.3.0 — GPL-2.0-or-later.** A port of SWORD with read/write support and Tantivy search. **Incompatible with our MIT/Apache plan** ([repo](https://github.com/loveJesus/rsword-chirho)). |
| Effort | **Low** (container) using `sword-modules`. **High** if reimplemented ourselves from its `docs/spec`. On top of that, **medium–high** for the markup layer (OSIS + ThML + GBF + TEI, tolerant of split tags). |
| Gotchas | The format may change with no notice. confs can lie about index width. Latin-1/CP1252 encodings. `@LINK` aliases. Strong's key zero-padding. Versification tables were extracted from libsword's runtime API: as facts they are probably not copyrightable, but **this is a legal judgement for us, not verified**. CrossWire asks apps to make network access an explicit user choice ([sword-modules](https://docs.rs/crate/sword-modules/latest)). That fits our offline-only rule if we support only local zip/folder import. |

### 1.4 Zefania XML

| Aspect | Facts |
|---|---|
| Spec | Elements `XMLBIBLE` → `BIBLEBOOK@bnumber/bname/bsname` → `CHAPTER@cnumber` → `VERS@vnumber`, plus `INFORMATION` (Dublin Core). There are also attributes for notes, references and grammar. Schemas `zef2005.xsd` and `zef2014.xsd` exist. **"Zefania XML is available under the GNU GPL"** ([preservation repo](https://github.com/biblenerd/Zefania-XML-Preservation), [de.wikipedia](https://de.wikipedia.org/wiki/Zefania_XML)). Official docs are at bgfdb.de/zefaniaxml/bml (linked from [SourceForge](https://sourceforge.net/projects/zefania-sharp/); **the page could not be fetched, so element details are unverified**). Combined verses are poorly specified (an open user question on SourceForge). |
| Structure | Container verses only, which is simple. Notes, cross-refs and Strong's (`gr` element, **unverified**) exist, but practical support varies by file. Quelea had to add tolerance for "non-conformant XML bibles" ([Quelea forum](https://quelea.discourse.group/t/unable-to-add-telugu-bible/683)). |
| Catalog | The SourceForge Bibles folder is organised by language and still active (updated 2026-09) ([files](https://sourceforge.net/projects/zefania-sharp/files/Bibles/)). There are mirrors on GitHub ([kohelet clone](https://github.com/kohelet-net-admin/zefania-xml-bibles)). **Licensing of individual files is unclear or mixed (unverified).** |
| Rust crates | No dedicated crate was verified. `quick-xml` is enough. |
| Effort | **Low** (about a day for a basic importer). Medium for robust handling of variants. |
| Gotcha | Do not vendor or ship the GPL XSD in our MIT/Apache repo. A parser written from observed structure should be independent. **This is a legal call, not verified.** |

### 1.5 theWord (.ont/.ot/.nt; books .twm) and MySword (.mybible)

| Aspect | theWord | MySword |
|---|---|---|
| Container | **Bibles are plain text**: "exactly 31,102 lines (for .ont files)", one verse per line in fixed KJV order, with custom HTML-like tags. Encrypted variants are `.ontx`/`.otx`/`.ntx` ([theWord manual](https://www.theword.net/manual/bible_modules.htm)). **Book modules are SQLite** ([Book Modules](https://www.theword.net/manual/book_modules.htm)). | SQLite with suffixes `.bbl/.cmt/.dct/.bok/.jor.mybible`, plus a `Details` metadata table. "Complete details will not be documented here" ([MySword format](https://mysword.info/modules-format)). |
| Tags | Linked spec on theword.gr ([manual](https://www.theword.net/manual/bible_modules.htm)). **Not fetched, so unverified.** | GBF-like tags "patterned after The Word": `<FR>…<Fr>` red letter, `<WG####>`/`<WH####>` Strong's, `<WT…>` morph, `<RF>…<Rf>` notes, `<RX43.3.16>` cross-refs, `<TS>…<Ts>` titles, `<CM>` paragraph. Also regex `VerseRules` and `CustomCSS` ([MySword format](https://mysword.info/modules-format)). |
| Versification | Fixed KJV line count, so non-KJV versification cannot be represented natively (inference from the 31,102-line rule). | Book.chapter.verse rows (schema is undocumented). |
| Catalog / licence | Many modules are free but copyrighted, and some are encrypted or paid ([manual](https://www.theword.net/manual/bible_modules.htm)). **Size of the freely licensed catalog is unverified.** | Same concern. **Unverified.** |
| Rust | No crate verified. Text parsing is trivial. | Needs SQLite: `rusqlite` bundles C SQLite. wasm32 support arrived via `sqlite-wasm-rs` in [rusqlite PR #1769](https://github.com/rusqlite/rusqlite/pull/1769) (merged 2025-12). |
| Effort | **Low** (Bible). Medium for books (SQLite + RTF/HTML). | **Low–medium.** |

### 1.6 ePub / plain text / VPL

- **ePub:** `rbook` 0.7.12 (Apache-2.0, ~67k downloads) reads EPUB 2 and 3 ([crates.io](https://crates.io/crates/rbook)). eBible ships an ePub 3 for each translation ([WEBP](https://ebible.org/find/details.php?id=engwebp)). A good fit for public-domain general books such as commentaries and classics. For Bibles it carries no verse semantics unless we parse its HTML anchors. Effort: low for reading, medium for verse alignment.
- **VPL (verse-per-line):** "Genesis 1:1 In the beginning…". CrossWire accepts it for text-only Bibles ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)). eBible ships `*_vpl.zip` ([WEBP](https://ebible.org/find/details.php?id=engwebp)). Effort: trivial. Combined verses are a problem ([DevTools:Modules](https://wiki.crosswire.org/DevTools:Modules)).

## 2. Comparison matrix

| Format | Spec quality | Notes / xrefs | Strong's / morph | Headings | Red letter | Versification | Free catalog | Permissive Rust option | Effort |
|---|---|---|---|---|---|---|---|---|---|
| USFM/USX | High, active | Yes / yes | Strong's+lemma; morph limited | Yes | `\wj` | Per text (+`altnumber`) | Very large (eBible) | `usfm3` MIT | Low–Med |
| OSIS | Good, frozen (2006) | Yes / yes | Rich parallel arrays | Yes | `q who="Jesus"` | osisRefWork | Moderate standalone | quick-xml, DIY | Med |
| SWORD | **None official** | Via markup | Via markup (KJV etc. tagged) | Via markup | Via markup | 18 systems | Large; per-module licences | `sword-modules` CC0 | Low container + Med–High markup |
| Zefania | Low–Med; GPL docs | Yes (varies) | Partial (unverified) | Some | Some (unverified) | Implicit | Moderate; licences unclear | DIY | Low |
| theWord | Low (partly linked) | Yes | Yes | Yes | Yes | KJV only | Unverified | DIY | Low |
| MySword | Low ("not fully documented") | Yes | Yes | Yes | `<FR>` | Unverified | Unverified | rusqlite (C) | Low–Med |
| ePub | High (W3C) | HTML only | No | HTML | No | None | Large (eBible, PD books) | `rbook` Apache-2.0 | Low |

## 3. Implications for our decisions (options, not choices)

1. **License gate:** Two crates are safe on licence for MIT/Apache: `sword-modules` (CC0) and `usfm3`/`usfm` (MIT). `rsword_chirho` (GPL-2.0+) and anything vendoring the Zefania XSD should be excluded. Before depending on `sword-modules`, read its `CLEANROOM.md` ourselves and decide whether its provenance is acceptable. The crate is 0.1.0 with one author (maturity risk).
2. **Starter library path:** The public-domain English texts (WEB, KJV, ASV) are all available as USFM from eBible and CrossWire ([File Formats](https://wiki.crosswire.org/File_Formats)). A USFM-only build pipeline could cover the bundled library. Note that Strong's-tagged KJV data is mostly distributed as OSIS/SWORD (KJV module example in [OSIS Bibles](https://wiki.crosswire.org/OSIS_Bibles)), so Strong's word study may push us toward OSIS or SWORD anyway.
3. **SWORD import pulls in several markup parsers** (OSIS-fragment, ThML, GBF, TEI). OSIS is reused from the OSIS importer. ThML and GBF are extra work. One option: v0.1 supports only `SourceType=OSIS` modules and rejects or flags the others.
4. **Internal model must be milestone-native.** USX 3 and OSIS both treat verses as milestones that overlap paragraphs. Our engine model should therefore store paragraph/section structure and verse ranges as separate overlapping layers, not nested verse containers.
5. **Versification** is cross-cutting. We need at least KJV plus a mapping table to align parallel translations. Possible sources include Paratext `.vrs` and the SWORD systems (via `sword-modules` data). Licensing of that data needs a check.
6. **WASM-clean engine:** XML and USFM importers in pure Rust are fine. SQLite-based formats (MySword, theWord books) need C SQLite or wasm shims. We could keep them out of the engine crate and behind a feature flag, or defer them.
7. **Low-value candidates for v0.1:** theWord and MySword have unclear free-catalog size and undocumented schemas. Zefania's licence status is murky and its data quality uneven. All three are cheap to add later.

## Gaps / unverified

- Exact licence of the `usfm-bible/tcdocs` spec ("Other"), the `logoxide-text` crate, and individual Zefania files.
- Zefania element reference (bgfdb.de fetch failed) and the theWord tag spec (theword.gr not fetched).
- Free-catalog counts for theWord, MySword and Zefania. The CrossWire figure covers the Bibles page only; commentaries and lexicons were not counted.
- Contents of `sword-modules`' `CLEANROOM.md` / `docs/spec` were not read; claims about them come from its README.
- Whether eBible still publishes OSIS for any translation (none listed for WEBP).
