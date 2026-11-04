# Starter Commentary and Dictionary converters (#44)

The converters in `crates/formats/src/commentary/` write through the shared
`ResourceBuilder`. `xtask/src/starter/study.rs` handles the pinned gzip/zip containers.
No UI changes or dependency on the general SWORD Import implementation (#48).
Commentary Tabs (#49) consume the existing Passage Keys and `scripture_ref` spans.

## Sources and decisions

- MHC and JFB use the exact sources, hashes and evidence chosen by
  [the #39 spike](../research/commentary-sources.md). MHC Attribution credits its
  Wikisource/CCEL lineage; its Changes note identifies **Luke 18:9–43** as missing.
- Easton 2.0.1 and Smith 1.3 use CrossWire-hosted archives. Their pinned archive
  configurations are archived in `starter/evidence/{easton,smith}.conf`; each grants
  public-domain distribution. Easton's upstream CCEL lineage is credited, but no
  CCEL-hosted bytes are fetched. No SWORD software is linked.
- Dictionary units use headwords, combining distinct senses with the same headword
  in source index order. Identical duplicate entries are removed and counted.
  Both Dictionaries add **Abram → Abraham** as a headword alias while retaining
  the original short Abram entry. Smith's legacy bytes are decoded as Windows-1252.
- MHC includes book and chapter introductions, but omits volume prefaces and indexes.
  The source's 32 footnotes are in those omitted volume prefaces; footnotes in a
  retained section are supported and tested. Three overlong section headings are
  bounded to the Standard chapter end: Prov 31:10–33 → 31, Isa 7:17–28 → 25,
  Acts 26:24–44 → 32. Prose is not corrected. The report lists these repairs.
- JFB omits front matter and navigation. Empty verse anchors share adjacent comments;
  section titles and their following lemmas stay together. Missing Mark 4:21 and
  Zech 1:2 anchors are restored from their own chapter's lemmas.
- Roman chapters, OnLine Bible abbreviations, OSIS references and unlinked prose
  references parse to Standard Passages. Invalid/out-of-canon link targets are counted,
  with samples in the report; readable display-text references can still be recognized.
  Italics, bold, headings, paragraphs, poetry and notes use the existing span vocabulary.
  Small caps and superscript presentation have no generic span kind: visible text is
  retained and unsupported presentation is counted, without changing the existing format version (now 2 after #43).

## Full-edition verification

Executed with the shared `cargo-slot` wrapper, using the committed source pins:

```sh
cargo xtask starter --check-determinism
cargo test -p xtask pinned_study -- --ignored --nocapture
cargo test -p obs-formats
cargo test -p xtask study_golden
cargo xtask ci
cargo xtask bench
```

Rebased onto #41 (`1bc035f`); combined lock includes OpenBible.info, TIPNR and six Lexicons.
All passed locally again, including the wasm check (the target was available in this run).
Two complete builds produced byte-identical output files. `dist/starter/` totals
**361.2 MB / 400 MB**, including the manifest/report; `.obr` files total 358.9 MB.
Search caches remain the separate #58 pipeline step and must also fit the budget.
Generated artifacts and full downloads are not committed.

| Resource | Units | Numbered Verses covered by sections | Scripture-reference spans | `.obr` MB |
|---|---:|---:|---:|---:|
| MHC | 5,477 | 31,061 | 63,612 | 62.6 |
| JFB | 19,774 | 31,102 | 40,419 | 28.9 |
| Easton | 3,961 | — | 24,067 | 7.8 |
| Smith | 4,561 | — | 15,004 | 6.8 |

Both Commentaries contain all 66 Bible books and 1,189 chapters. MHC's section-only
coverage excludes introductions: apart from Luke 18:9–43, the six remaining Verses
are discussed in introductions or neighbouring sections (the heading slips from #39).
Unparsed explicit target counts: MHC 706, JFB 20, Easton 29, Smith 152. They are
reported rather than silently turned into invalid references.

## Regression tests

- `study_golden_sections_open_by_passage_and_dictionary_alias`: four real-source
  golden text/span snapshots; writes `.obr`, opens each Commentary by its exact
  Passage Key, and resolves each Dictionary's Abraham both directly and via Abram.
- `pinned_study_editions_have_complete_coverage_and_stable_bytes`: opt-in full cache
  test; verifies hashes, complete chapter coverage, JFB's 31,102 Verses, MHC's known
  gap/Attribution, every reference target, and independent byte-identical rebuilds.
- `the_real_lock_passes_the_provenance_gate`: all sixteen actual source entries,
  archived evidence hashes and the no-CCEL-host rule. No doc-coupled temporary test.
- `commentary::tests`: Roman/contextual reference parsing, UTF-8 span offsets,
  unlinked prose, malformed XML/offsets, notes and poetry, duplicate anchors,
  leading empty anchors and chapter-scoped missing-anchor repair.
- PR #100 review regressions: protect Micah's abbreviation before Roman conversion;
  preserve chapter-relative continuations such as `Ps. cxxi. 2; cxxiv. 8` in one
  Scripture-reference span; convert line-leading wiki indentation independently of
  blank-line paragraph separation. Four new tests failed before the fixes and pass
  afterward, including assertions against the real MHC excerpt. The refreshed MHC
  golden removes only indentation colons from text, updates byte offsets, and adds
  Psalm 124:8 to the existing reference's targets. Full CI (including wasm), the
  deterministic sixteen-Resource build, full pinned-edition test and benchmark passed
  again after these fixes.

- Rendering-review regressions: fallback prose excludes heading-covered ranges;
  explicit breaks split paragraph spans while preserving poetry style and inline
  markup. Persisted Easton Abraham retains its heading in `unit_blocks`; the real
  JFB golden renders all five source lines independently. Synthetic JFB paragraphs
  (including leading, repeated and trailing breaks) and two-line MHC poetry stay
  separate at 100,000 px layout width. Golden text and inline spans are unchanged:
  only Easton's duplicate paragraph and JFB's structural paragraph ranges change.
  Full local CI (including wasm), pinned-edition tests, deterministic Starter build
  and benchmark passed again; current sizes above include these structural fixes.

Small public-domain excerpts, provenance notes and golden JSON live under
`crates/formats/tests/fixtures/study/`; ordinary CI requires no downloads.
To deliberately regenerate reviewed snapshots, set `UPDATE_STUDY_GOLDENS=1` when
running the golden test, then inspect the text and span diff before committing.
