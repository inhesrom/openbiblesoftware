# Compact `.obr` span storage (format 3)

Preparation for #40, not the TAHOT/TAGNT converter. Baseline: master `255eb32`.
ADR 0003 / #11's logical model is unchanged: each standoff span still has its unit,
kind, start/end UTF-8 byte offsets and attrs. Only `obs-store` knows the physical
encoding. No dependencies, source pins, provenance, importer kinds or content changed.

## Layout

`spans` remains a WITHOUT ROWID table with primary key `(unit, seq)` and index
`spans_range(unit, start, end)`. `seq` preserves canonical ordering. `kind` changes
from TEXT to INTEGER; `attrs` changes from nullable JSON TEXT to a non-null BLOB.
Attribute-free kinds have an empty BLOB. Integer ids are explicit, not Rust enum
ordinals or `SpanKind::rank()`.

Every integer inside attrs is canonical unsigned LEB128 (at most 32 bits). A string
is its UTF-8 byte length followed by those bytes. An optional string uses length + 1,
with zero meaning `None`; `Some("")` is distinct. A Verse is three integers: zero-based
Bible book index (Genesis = 0), chapter, verse. Chapter/verse/last fit u16; heading
level fits u8. A Passage is its start Verse followed by its end Verse.

| Kind id | Logical kind | Attrs in order |
|---|---|---|
| 0 | verse | Verse, last |
| 1 | chapter | chapter |
| 2 | para | style: prose=0, poetry1=1, poetry2=2, poetry3=3, list=4, blank=5 |
| 3 | heading | level |
| 4 | tagged_word | part count; each part: Strong's string, optional lemma, optional morph, optional gloss |
| 5 | note_ref | UnitId string, note kind (footnote=0, cross-reference=1) |
| 6 | xref | Passage count, then ordered Passages |
| 7 | scripture_ref | Passage count, then ordered Passages |
| 8–16 | woj, added, divine_name, emphasis, italic, bold, foreign, selah, psalm_title | empty |

Strings are not normalized again, morpheme parts and Passage targets are not sorted
or deduplicated, and extended Strong's numbers retain their homograph letter. The
reader rejects unknown ids, invalid enums/UTF-8/identifiers, reversed Passages,
overflowing or overlong integers, truncation, impossible counts and trailing bytes.
It never preallocates from an untrusted count. Semantic Resource validation remains
in the existing builder/load path.

`FORMAT_VERSION` increases from 2 to 3; the existing typed older/newer-version
errors require re-import/rebuild, not in-place migration. Format version already
participates in the logical content hash. Golden JSON diffs therefore change only
hashes, never text or spans. No change to the hash algorithm or model serde format.

## Alternatives and reader audit

All SQL span access is in `obs-store::obr`: writer insertion, summary count and
`spans_where`. `unit()` reads all spans in canonical order; `spans_in()` exposes
range/point queries including enclosing spans and zero-width markers at both edges.
Current callers primarily fetch units, but packing an entire Bible book into one
BLOB would require decoding all its spans for the existing range API. Retain rows
and the range index rather than introducing a new chunk/cache layer or regressing
that API. No indexes were removed. The writer already VACUUMs after its transaction.

Measured ASV table sizes (SQLite `dbstat`, bytes):

| Object | Format 2 | Format 3 |
|---|---:|---:|
| spans | 46,321,664 | 21,909,504 |
| spans_range | 12,386,304 | 12,386,304 |

ASV page-size/VACUUM experiments on copies of format 3:
1024 → 41,808,896 bytes; **4096 → 41,222,144**; 8192 → 41,271,296;
16384 → 41,533,440. Keep 4096. Warm Python/SQLite diagnostic timings (10 executions,
not a performance gate): Genesis full-row query 25.36 → 20.80 ms; point query at
byte 20,000 0.82 → 0.80 ms. Same SQL predicates/index as production; no latency tradeoff
is needed for these savings.

## Full Starter measurements

`cargo xtask starter --out dist/starter-before` on baseline, then
`cargo xtask starter --check-determinism` with format 3. Decimal MB, all sixteen
current master Resources, from the generated build reports/manifests:

| Resource | Before MB | After MB | Reduction |
|---|---:|---:|---:|
| abbott-smith | 11.932 | 6.443 | 46.0% |
| asv | 65.634 | 41.222 | 37.2% |
| bdb | 6.521 | 5.448 | 16.5% |
| bsb | 13.505 | 10.506 | 22.2% |
| easton | 7.832 | 4.858 | 38.0% |
| jfb | 28.860 | 21.176 | 26.6% |
| kjv-strongs | 39.666 | 26.227 | 33.9% |
| mhc | 62.570 | 52.617 | 15.9% |
| openbible-xrefs | 14.336 | 14.336 | 0.0% |
| smith | 6.840 | 4.743 | 30.7% |
| strongs-greek | 4.907 | 3.990 | 18.7% |
| strongs-hebrew | 6.353 | 5.435 | 14.4% |
| tbesg | 17.920 | 11.047 | 38.4% |
| tbesh | 5.259 | 4.604 | 12.5% |
| tipnr | 1.925 | 1.925 | 0.0% |
| web | 64.819 | 40.710 | 37.2% |
| **Total `.obr`** | **358.879** | **255.287** | **28.9%** |

Including build report, manifest and Changes files: **361.2 → 257.6 MB**, saving
**103.6 MB**. ASV/WEB exceed the 35% span-heavy-file target; prose-heavy MHC does not.
Conservatively adding the supplied *uncompacted* TAHOT (62.5 MB) and TAGNT (22.6 MB)
measurements gives **342.7 MB**, leaving **57.3 MB** of the 400 MB budget for Search
caches. Their format-3 files should be smaller, but this task does not claim a
measured eighteen-Resource total. #40 must rebuild on this writer; #58 must measure
actual caches against the same combined budget.

## Reproduction and tests

All local Cargo commands ran through the shared `lanes/cargo-slot` semaphore.
Generated full downloads and `.obr` artifacts remain uncommitted.

- `cargo test -p obs-store`: every closed-vocabulary kind, all paragraph/note kinds,
  multi-part Hebrew/Greek Tagged words, optional/empty strings, extended Strong's,
  multi-/duplicate/empty Passage lists, wire goldens, malformed attrs, every
  truncation, Resource round-trip, typed version refusal and deterministic overwrite.
- `compact_spans_preserve_every_range_query_and_use_typed_columns`: compares SQL
  results with logical predicates across every sample byte, reversed/point ranges,
  zero-width boundaries and extreme offsets. Checks INTEGER/BLOB column types.
- `cargo test -p obs-model -p obs-formats`: logical golden snapshots; only format-seeded
  content hashes change. Existing study golden snapshots remain unchanged.
- `cargo xtask starter --check-determinism`: two independent full builds are
  byte-identical, including reports/manifests.
- `starter_format_two_and_three_have_identical_logical_content`: full-build opt-in
  comparison. Loads/validates every format-3 Resource, rehashes it with version 2
  to match the old content hash, and compares every text and decoded span directly
  against format-2 SQL rows. All sixteen pass. Run with:

```sh
OBS_COMPACT_BEFORE="$PWD/dist/starter-before" \
OBS_COMPACT_AFTER="$PWD/dist/starter" \
cargo test -p obs-store starter_format_two_and_three -- --ignored --nocapture
```

Full `cargo xtask ci` passed: fmt, layers, assets, clippy, workspace tests/doctests
and wasm. Despite the initial host warning, the wasm target was available in this run.
`cargo xtask bench` also passed: 0% missed frames, origin jump 0 frames, all panes
1 frame, UI layout p99 0.85 ms, text GPU 1.0 MiB.

No UI change; visual evidence is not applicable.
