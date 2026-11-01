# Search and storage options in Rust (issue #6)

**Question:** Which storage and search stack fits a local Bible library in Rust? Compare tantivy, SQLite (rusqlite `bundled`) + FTS5, and a custom inverted index; note milli/Meilisearch and sled/redb/fjall KV stores.

**Date:** retrieved late 2026 (newest cited source: AndBible roadmap dated 2026-10-09). Crate versions seen: rusqlite 0.41.x (bundles SQLite 3.54.0), tantivy 0.27.0, redb 4.1.0, fjall 3.1.13.

## TL;DR

- **SQLite + FTS5 (via rusqlite `bundled`)** covers phrase, `NEAR`, boolean, prefix, column filters, BM25, `highlight()`/`snippet()`, and fast **rowid-range scoping**: `WHERE rowid BETWEEN ? AND ?` uses the doclist index. That maps directly to "Romans 1–8" if rowid = verse ordinal. The same file can also hold notes, highlights and other relational data. SQLite is public domain; rusqlite is MIT.
- **FTS5 diacritic folding covers Latin script only.** Greek/Hebrew accent and pointing folding must be done in Rust before insert (store a normalized shadow column), or with a custom FTS5 tokenizer through the C API. Porter stemming is English-only.
- **tantivy** (MIT) is the strongest pure-Rust engine. It has a Lucene-style query parser (AND/OR/+/-, phrases, `~slop`, ranges, `IN`), a `SnippetGenerator`, stemmers for 18 languages (Snowball Greek is *modern* Greek), startup under 10 ms, and incremental commits. **wasm32 is not an official target.** The wasm RFC #541 has been open since 2019, default features pull mmap/fs deps, and it uses rayon/crossbeam threads.
- **WASM:** rusqlite now defaults to `ffi-sqlite-wasm-rs` on `wasm32-unknown-unknown`, with an OPFS-backed `sahpool` VFS available. It is single-threaded (`SQLITE_THREADSAFE=0`). FTS5 being enabled in that build is **unverified**; `SQLITE_WASM_RS_FLAGS` can add flags.
- **Custom inverted index** (e.g. postings of verse-ordinals per token/Strong's, stored in redb or flat files) is the most WASM-clean and Bible-specific option. You write the query parser, phrase/proximity, ranking and snippets yourself.
- **KV stores:** redb (MIT/Apache, "file format is stable"), fjall (MIT/Apache, LSM), sled ("beta… on-disk format is going to change"). These are useful as a backing store for a custom index, not as search engines.
- **milli/Meilisearch:** not a fit. milli is `publish = false` inside the Meilisearch workspace. The repo license is `MIT AND BUSL-1.1` (Enterprise parts).
- **Prior art:** SWORD/BibleTime/Xiphos use CLucene. JSword (AndBible, STEP) uses Lucene with separate `strong`, `morph`, `contentStem` fields. **AndBible plans to replace Lucene with SQLite FTS5** in its shared Android/iOS codebase.

---

## 1. Requirements → capability mapping

| Requirement | SQLite FTS5 | tantivy | Custom index |
|---|---|---|---|
| Verse/range scope (Rom 1–8) | rowid = verse ordinal; `rowid BETWEEN` uses the doclist index [fts5] | fast field (u64 ordinal) + range query [qp] | Native: postings are sorted ordinals, so you binary-search the range |
| Strong's / lemma search | Separate indexed column (`strong:G26`) with column filter [fts5] | Separate `raw`-tokenized field [tok] | Separate postings keyed by Strong's |
| Phrase / proximity | `"a b"`, `a + b`, `NEAR(a b, N)` [fts5] | `"a b"`, `"a b"~N` slop [qp] | DIY (positional postings) |
| Boolean | AND/OR/NOT, parentheses [fts5] | AND/OR, `+`/`-`, `IN` [qp] | DIY |
| English stemming | `porter` wrapper tokenizer (English only) [fts5] | `en_stem` / `Stemmer(Language::English)` [tok] | Add a stemmer crate (e.g. rust-stemmers; not evaluated here) |
| Greek/Hebrew tokenization & diacritic-insensitive | unicode61 tokenizes them; `remove_diacritics` covers **Latin script only** [fts5]. Pre-normalize in Rust or write a custom tokenizer | Custom `TokenFilter`. `AsciiFoldingFilter` targets non-ASCII→ASCII equivalents, so it doesn't strip Greek accents (behavior on Greek **unverified**) [tok] | Full control (NFD + strip combining marks) |
| Highlight / snippets | `highlight()`, `snippet()`, `bm25()` [fts5] | `SnippetGenerator` → HTML [snip] | DIY (token offsets are needed anyway for verse rendering) |
| Incremental indexing (notes) | Normal INSERT/UPDATE/DELETE; automerge b-trees [fts5] | `IndexWriter` commit + reader reload; docs immutable, so delete+reindex [tantivy] | DIY (simple for small notes corpus) |
| Startup | Open file; no warm-up (no benchmark found) | "<10ms" [tantivy] | Depends on design (mmap/redb opens fast) |
| WASM | rusqlite → sqlite-wasm-rs, OPFS VFS [rusqlite][swr] | Not officially supported [#541] | Yes, if built on pure-Rust deps |
| License | SQLite public domain; rusqlite MIT [rusqlite] | MIT [cargo] | Ours |

Sources: [fts5](https://sqlite.org/fts5.html), [qp](https://docs.rs/tantivy/latest/tantivy/query/struct.QueryParser.html), [tok](https://docs.rs/tantivy/latest/tantivy/tokenizer/index.html), [snip](https://docs.rs/tantivy/latest/tantivy/snippet/index.html), [tantivy](https://github.com/quickwit-oss/tantivy), [cargo](https://github.com/quickwit-oss/tantivy/blob/main/Cargo.toml), [#541](https://github.com/quickwit-oss/tantivy/issues/541), [rusqlite](https://github.com/rusqlite/rusqlite), [swr](https://docs.rs/sqlite-wasm-rs/latest/sqlite_wasm_rs/).

## 2. SQLite + FTS5 (rusqlite)

**Facts**
- **Build and license.** `bundled` compiles vendored SQLite (currently 3.54.0 as of rusqlite 0.41.0 / libsqlite3-sys 0.39.0) and links it in. SQLite is public domain; rusqlite and libsqlite3-sys are MIT. `LIBSQLITE3_FLAGS` can add or remove compile flags. ([rusqlite README](https://github.com/rusqlite/rusqlite))
- **FTS5 in `bundled`.** The bundled build enables FTS5 per [libsqlite3-sys build.rs](https://github.com/rusqlite/rusqlite/blob/master/libsqlite3-sys/build.rs). I didn't re-read this line by line, so treat it as **verify with a `CREATE VIRTUAL TABLE … USING fts5` test**.
- **Query syntax.** Phrases, `NEAR` groups, `^` initial-token, column filters, AND/OR/NOT, prefix `*`. ([fts5 §3](https://sqlite.org/fts5.html))
- **Tokenizers.** `unicode61` (default), `ascii`, `porter` (wrapper; "designed for use with English language terms only"), and `trigram` (substring/LIKE/GLOB). ([fts5 §4.3](https://sqlite.org/fts5.html))
- **Diacritics.** unicode61 `remove_diacritics` 1/2 strips diacritics "from Latin script characters". It doesn't cover Greek polytonic or Hebrew niqqud. `categories` can add `Mn` so combining marks are kept *inside* tokens rather than splitting them. ([fts5 §4.3.1](https://sqlite.org/fts5.html))
- **Custom tokenizers** (C API `xCreateTokenizer_v2`) support synonyms, which could be used to emit lemma/Strong's alongside surface forms. `tokendata=1` supports asymmetric (unmarked-query vs marked-text) matching. ([fts5 §4.7, §7.1](https://sqlite.org/fts5.html))
  - **Unverified:** whether rusqlite exposes a safe Rust API for registering FTS5 tokenizers. Its `vtab` feature covers read-only virtual tables, not tokenizers ([rusqlite README](https://github.com/rusqlite/rusqlite)). Fallback: normalize in Rust and insert pre-folded text into the indexed column, keeping the display text in an external content table.
- **Range scoping.** "`SELECT ... FROM ft('term') WHERE rowid BETWEEN ? AND ?` — FTS5 uses … its doclist index to efficiently identify the subset of matches with rowids in the required range." ([fts5 §9](https://sqlite.org/fts5.html))
- **Storage modes.** External-content and contentless tables avoid duplicating text. `detail=full|column|none` trades phrase/NEAR capability for size. In SQLite's email test, a 1636 MiB corpus gave an FTS index of 743 MiB (full), 340 MiB (column) and 134 MiB (none). ([fts5 §4.4, §4.6](https://sqlite.org/fts5.html))
- **Writes.** Each transaction writes a new segment b-tree, auto-merged (`automerge` default 4). `optimize` merges everything into one structure. ([fts5 §6](https://sqlite.org/fts5.html))
- **WASM.** The rusqlite default feature `ffi-sqlite-wasm-rs` "switches to using the `sqlite-wasm-rs` crate … on `wasm32-unknown-unknown`" ([rusqlite README](https://github.com/rusqlite/rusqlite)). sqlite-wasm-rs provides a memory VFS and an OPFS `sahpool` VFS. It notes "Multithreading is not supported, SQLite is compiled with `-DSQLITE_THREADSAFE=0`", and `SQLITE_WASM_RS_FLAGS` adds compile flags. ([sqlite-wasm-rs docs](https://docs.rs/sqlite-wasm-rs/latest/sqlite_wasm_rs/))
  - **Unverified:** whether FTS5 is on by default in that build.

**Trade-offs.** SQLite is one engine for both relational data (resources, verses, notes, highlights, cross-refs) and search. It is extremely mature, and the C dependency is public-domain and allowed under our MIT/Apache rule. It is not "pure Rust": it needs a C toolchain via `cc`, which is fine on desktop and handled on WASM by sqlite-wasm-rs. Greek/Hebrew folding needs our own normalization. Ranking customization is limited without C-API auxiliary functions.

## 3. tantivy

**Facts**
- **License and maturity.** MIT license, v0.27.0, pre-1.0 ([Cargo.toml](https://github.com/quickwit-oss/tantivy/blob/main/Cargo.toml)). The README describes a "Lucene-inspired" crate with BM25, phrase queries, incremental indexing, mmap directory, a compressed doc store, faceting, and "Tiny startup time (<10ms)". It reports indexing English Wikipedia in "< 3 minutes". ([README](https://github.com/quickwit-oss/tantivy))
- **Query parser.** AND/OR, `+`/`-`, phrases, `"big wolf"~1` slop, `"big bad wo"*` phrase-prefix, ranges `[a TO c}`, `IN [..]`, field boosts, fuzzy per field. ([QueryParser docs](https://docs.rs/tantivy/latest/tantivy/query/struct.QueryParser.html))
- **Tokenizers.** `default`, `raw`, `en_stem`, plus building blocks: `SimpleTokenizer`, `RegexTokenizer`, `NgramTokenizer`, `LowerCaser`, `AsciiFoldingFilter`, `Stemmer`, `StopWordFilter`, and a custom `Tokenizer`/`TokenFilter` trait (implement against `tantivy-tokenizer-api`). ([tokenizer docs](https://docs.rs/tantivy/latest/tantivy/tokenizer/index.html))
  - The stemmer crate's features include `english` and `greek` ([Cargo.toml](https://github.com/quickwit-oss/tantivy/blob/main/Cargo.toml)). Snowball Greek targets modern Greek; its usefulness for Koine is **unverified**.
- **Snippets.** `SnippetGenerator::snippet_from_doc` → `to_html()`. ([snippet docs](https://docs.rs/tantivy/latest/tantivy/snippet/index.html))
- **Updates.** Docs are immutable (edits mean delete + reindex). Changes become visible after `commit` and a reader reload. ([README FAQ](https://github.com/quickwit-oss/tantivy))
- **WASM.**
  - Default features are `mmap` (fs4, tempfile, memmap2), `stemmer`, `lz4-compression`, and others. Unconditional deps include `rayon` and `crossbeam-channel`. ([Cargo.toml](https://github.com/quickwit-oss/tantivy/blob/main/Cargo.toml))
  - The wasm RFC [#541](https://github.com/quickwit-oss/tantivy/issues/541) is still **open**. The maintainer noted the smallest build was ~900 KB "with a lot of cheating & trimming".
  - Upstream removed some blockers: async indexer code ([PR #1312](https://github.com/quickwit-oss/tantivy/pull/1312)) and the `fail` dep made dev-only ([PR #2094](https://github.com/quickwit-oss/tantivy/pull/2094)).
  - A third-party fork reports compiling with `Index::create_in_ram` and no mmap for `wasm32-unknown-unknown` ([cozo-redb commit](https://github.com/lawless-m/cozo-redb/commit/cb6e9256b3f49c585e42c697e6705baef92d2686)). This is third-party evidence, not upstream support.
  - Persistence in the browser would need a custom `Directory` implementation (e.g. over OPFS/IndexedDB). That is **not evaluated**.

**Trade-offs.** tantivy gives the best query language and ranking out of the box and is pure Rust. Its index lives in a separate directory, so we still need a store for notes and metadata (e.g. SQLite or redb), which means two persistence systems. There's no official WASM path and the format is pre-1.0. The format changing across versions is a reason to keep indexes as rebuildable caches, not shipped artifacts (inference, not verified against tantivy's compat policy).

## 4. Custom inverted index

**What it means.** At import time, tokenize each resource into (token → sorted list of `(verse_ordinal, position)`). Add extra keyspaces for `strong:G0026`, `lemma:ἀγάπη`, and morph codes. Store postings in redb tables or in flat, read-only, memory-loaded files per resource.

- **Range scope:** binary-search postings by ordinal. **Phrase/proximity:** positional intersection. **Boolean:** set ops on sorted lists.
- **Snippets:** we already have verse text and token offsets.
- **Size, rough estimate (unverified):** KJV-sized Bibles are ~790k tokens. Positional postings at ~2–4 bytes/posting delta-encoded gives a few MB per Bible. 30–100 MB of text would likely yield tens of MB of index. Measure before deciding.
- **Build time:** this is single-pass, in-memory work. **No benchmark found.** Prototype it.
- **Pros:** fully WASM-clean, Bible-native (verse ordinals, versification mapping, Strong's/morph as first-class), no format-churn risk, deterministic, and small.
- **Cons:** we own the query parser, ranking, stemming integration, merge/update logic and bugs. Commentaries and books (non-verse keyed) need a second key scheme.

**KV stores (if a custom index needs a backing store):**

| Store | License | Status (per README) | Notes |
|---|---|---|---|
| redb 4.1.0 | MIT OR Apache-2.0 | "Stable and maintained. The file format is stable" | ACID, copy-on-write B+trees, MVCC, pure Rust ([GitHub](https://github.com/cberner/redb), [crates.io](https://crates.io/crates/redb/4.1.0)). WASM support **unverified** (README doesn't mention it) |
| fjall 3.1.13 | MIT OR Apache-2.0 | LSM, "100% safe & stable Rust" | Depends on `tempfile`, a likely WASM hurdle (**unverified**) ([Cargo.toml](https://github.com/fjall-rs/fjall/blob/main/Cargo.toml), [README](https://github.com/fjall-rs/fjall)) |
| sled | MIT/Apache (per crate) | "sled is beta… on-disk format is going to change… before 1.0.0" | Not recommended for persisted user data ([README](https://github.com/spacejam/sled)) |

## 5. milli / Meilisearch

- milli is the internal Meilisearch engine. It "can manage one and only one index" and "is unable to store updates in a store" ([milli README](https://github.com/meilisearch/meilisearch/blob/master/crates/milli/README.md)). It is `publish = false` (not on crates.io) ([Cargo.toml](https://github.com/meilisearch/meilisearch/blob/ef3fab01/crates/milli/Cargo.toml)).
- The repo license is `MIT AND BUSL-1.1`, with Enterprise Edition parts under BUSL ([LICENSE](https://github.com/meilisearch/meilisearch/blob/HEAD/LICENSE)). Its focus is typo-tolerant, search-as-you-type relevance, not positional/phrase/proximity scholarly queries (characterization; not deeply verified).
- Conclusion from the facts: high integration and licensing friction, poor fit.

## 6. How other open Bible apps search

| App | Engine | Evidence |
|---|---|---|
| SWORD engine (used by BibleTime, Xiphos, PocketSword) | Optional CLucene "fast search" indexes (`mkfastmod`) plus built-in regex/phrase/multiword scans | [SWORD INSTALL](https://crosswire.org/svn/sword/trunk/INSTALL), [SWORD dev page](https://crosswire.org/sword/develop/index.jsp), [CrossWire compare table](https://wiki.crosswire.org/Choosing_a_SWORD_program) |
| BibleTime | Requires CLucene 2.3.3.4+ and SWORD; its own CLucene index | [README](https://github.com/bibletime/bibletime/) |
| JSword (AndBible Bibles, STEP) | Lucene. Fields: `key` (osisID), `content`, `contentStem`, `strong`, `morph`, `heading`, `xref`, `note`, `intro`. Field names "named the same as Sword in the hopes of sharing indexes" | [LuceneIndex.java](https://github.com/crosswire/jsword/blob/master/src/main/java/org/crosswire/jsword/index/lucene/LuceneIndex.java) |
| AndBible | Lucene via JSword for Bibles; SQLite FTS5 for EPUB (raw FTS5 syntax exposed in UI). Roadmap: "Search: Lucene is replaced by SQLite FTS5" in the shared KMP codebase, with bundled SQLite | [2023 EPUB post](https://andbible.org/2023/12/18/new-feature-now-released-searching-for-epub-e-book-documents/), [Q4 2026 roadmap](https://andbible.org/2026/10/09/the-road-ahead-q4-2026-q1-2027/) |
| STEPBible | Server-side Lucene (Java). Searches run per version, then verse key-sets are merged | [STEP Searches wiki](https://stepweb.atlassian.net/wiki/spaces/TYNSTEP/pages/5799948/Searches), [STEP repo](https://github.com/STEPBible/step); Lucene usage seen in `LuceneUtils.java` via a [mirror](https://www.javatips.net/api/step-master/step-core/src/main/java/com/tyndalehouse/step/core/utils/LuceneUtils.java) (secondary) |

**Pattern:** every mature app indexes **one document per verse key**, with separate fields for stemmed text, Strong's and morphology. Ranges are then a key filter. Licensing note: SWORD is GPL and JSword is LGPL (header in LuceneIndex.java), so we can learn from their field design but not link to them.

## 7. Options matrix

| Criterion | A: SQLite+FTS5 only | B: tantivy + SQLite/redb for data | C: Custom index on redb/flat files | D: Hybrid (FTS5 now, custom Strong's/morph postings) |
|---|---|---|---|---|
| Range scope | ★★★ rowid range | ★★★ fast-field range | ★★★ native | ★★★ |
| Strong's/lemma | ★★ column | ★★★ raw field | ★★★ first-class | ★★★ |
| Phrase/NEAR/boolean | ★★★ built-in | ★★★ richest parser | ★ DIY | ★★★ |
| English stemming | ★★ Porter | ★★★ Snowball | ★ add crate | ★★ |
| Greek/Hebrew folding | ★ pre-normalize | ★★ custom filter | ★★★ | ★★ |
| Snippets/highlight | ★★★ | ★★★ | ★ DIY | ★★★ |
| Notes incremental | ★★★ same DB | ★★ commit/reload | ★★ DIY | ★★★ |
| Startup | ★★★ (no benchmark found) | ★★★ <10 ms | ★★★ | ★★★ |
| WASM | ★★ sqlite-wasm-rs, OPFS, FTS5 flag unverified | ★ unofficial | ★★★ | ★★ |
| One storage system | ★★★ | ★ two | ★★ | ★★★ |
| Pure Rust | ✗ (C, public domain) | ✓ | ✓ | ✗ |
| License fit (MIT/Apache) | ✓ | ✓ | ✓ | ✓ |
| Maturity | ★★★ | ★★ pre-1.0 | ★ new code | ★★★ |
| Our effort | Low | Medium | High | Medium |

★ ratings are a qualitative synthesis of the cited facts, not measured benchmarks.

## 8. Implications for our decisions (facts to weigh, not a decision)

1. **Engine boundary.** If `crates/engine` must stay WASM-clean, search should sit behind a trait (e.g. `SearchIndex { query(scope, q) -> VerseHits }`) so the backend can differ between desktop and web.
2. **Verse ordinal as primary key** (book·chapter·verse packed into an integer, per versification) makes range scoping cheap in *all* options. It also matches what JSword/SWORD do with `key`.
3. **Normalization is ours regardless.** No option folds Greek/Hebrew diacritics out of the box. A shared Rust normalizer (NFD → strip `Mn` → lowercase/final-sigma fold) used at both index and query time works with any backend.
4. **Indexes are caches.** Build them locally from our resource format (AndBible rebuilds indexes on engine change). Avoid shipping engine-specific index files, especially tantivy's pre-1.0 format.
5. **User notes favor SQLite** if SQLite is already the data store: transactional, incremental FTS, and one file to back up.
6. **License check passes** for SQLite, rusqlite, tantivy, redb and fjall. Excluded: libsword (GPL), JSword (LGPL; Java anyway), and milli (unpublished; repo `MIT AND BUSL-1.1`).

## Gaps / next steps

- **Benchmarks.** No primary data on index size or build time for our ~30-resource, 30–100 MB corpus in any engine. Next step: a spike indexing KJV + one commentary in FTS5 (`detail=full`) and in tantivy, measuring size, build time and cold-open time.
- **Unverified items:**
  - FTS5 enabled in the default sqlite-wasm-rs build
  - Exact `SQLITE_ENABLE_FTS5` line in libsqlite3-sys build.rs
  - rusqlite API for custom FTS5 tokenizers
  - redb/fjall on `wasm32-unknown-unknown`
  - `AsciiFoldingFilter` behavior on Greek
  - Snowball Greek on Koine
- **Not covered:** Hebrew prefix and suffix segmentation (ו/ה/ב), and whether lemma search should come from tagged data (e.g. STEPBible TAGNT/TAHOT, CC BY 4.0 per [STEPBible-Data](https://github.com/STEPBible/STEPBible-Data/)) rather than tokenization. The latter is a data-model question, likely a separate ticket.
