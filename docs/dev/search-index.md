# Search index caches (#58)

Every Resource gets one Search index: a SQLite FTS5 file kept apart from its `.obr` (ADR 0003). It is derived data. Deleting it is always safe, because the app rebuilds it in the background. The query grammar and the search UX are decided in #16. The latency targets come from #19.

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `SearchIndex`, `SearchHit`, `SearchOptions`, `INDEX_VERSION` | The seam: what one Resource's index answers. It does not care how the index is stored. |
| `obs-store` | `build_search_index`, `SqliteSearchIndex`, `IndexKey` | The FTS5 cache file: building it (with progress and cancellation), opening it and querying it. |
| `obs-engine` | `search` (`SearchIndexes`, `CacheIndexer`, `Session::search`) | Status for each Resource (Indexing / Ready / Failed / Unavailable) and a background rebuild queue. It also provides the Import indexer and runs searches over the indexes that are ready. |
| `obs-ui-egui` | `App::with_file` | Calls `Session::set_search_cache_dir` at startup (this also installs the Import indexer), then `refresh_search_indexes`. The Import report shows the live index status. |
| `obs-cli` | `index`, `search` | Builds caches and times queries. |
| `xtask` | `starter` | Writes prebuilt caches to `dist/starter/search/`. |

## The cache file

- **Key.** A cache is keyed by `(resource_id, content hash, index version)`, and the key is the file name: `<resource_id>.<16 hex digits of the content hash>.v<INDEX_VERSION>.obsidx`. A resource id that is not `[a-z0-9-]{1,64}` (a dropped `.obr` is not checked) is replaced by `_` and a hash of it, so a name never leaves the cache folder or matches another Resource's files. The same key is also stored in its `meta` table and checked when the file is opened. A new revision of the text or a new index version therefore means a different file name. The old file is deleted once the new one has been built. Each build writes to its own temp file (`<name>.<pid>-<n>.partial`) and renames it into place, so two builds of the same key never collide.
- **Documents.** In a Bible, each Verse is one document (in the Bible's own numbering), and its id is the verse ordinal, so a Search scope becomes `rowid BETWEEN`. Headings inside a Verse's range are not indexed. In any other Resource, each `para` span is one document, and so is each stretch of text outside a paragraph that contains a word (headings, entries with no paragraphs). Each document carries its unit's id, its title and the Key that covers the paragraph.
- **Fields.** `text` holds the normalized words. `strong` holds the Strong's numbers of the morpheme parts in the document's Tagged words, in extended and base forms. `lemma` holds the normalized Lemmas. The table uses `detail=full`, so phrase queries work.
- **External content.** `docs` records where each document sits in the Resource (unit and byte range). The indexed text itself is never stored. Hits are highlighted in Rust on the display text (#16 rule 7), so FTS5 never needs the text back.
- **Determinism.** One `optimize`d segment followed by `VACUUM`. The same Resource and version always produce the same bytes.

## Lifecycle

1. **Import** builds the cache during its Index phase, with real progress in the Import report. Cancelling the Import stops the build and leaves no file behind.
2. **At startup and after an Import**, every library Resource is checked. The app looks for a cache with the current key first in the cache folder, then in `search/` beside the `.obr` (where the Starter library's prebuilt caches are). A file with the right name that does not open as that key (truncated, foreign, mismatched) is deleted if it is in the cache folder and skipped otherwise. If no usable cache exists, the Resource is queued. The queue builds **one index at a time**, so rebuilds never occupy every job worker. While builds are queued or running, the app redraws every 250 ms even when idle, because job results are applied by a frame.
3. **While a Resource is rebuilt** after an index version change, its older cache (same content, another version, from either folder) keeps being searched. Those Resources are listed in `SearchResults::stale` (and `search.indexes` reports the old file as `stale`). When the new cache is Ready it is swapped in and the old file is deleted from the cache folder. A cache of different content is never used, because its offsets point into the old text.
4. **During a search**, Ready and stale indexes are searched. Resources with neither are listed in `SearchResults::indexing`. If a cache file has been deleted or damaged underneath the app, that Resource is re-queued and the search goes on with the others. A Failed build is retried by the next search, once per session; `refresh_search_indexes` always retries.
5. **Removing a Resource** from the library deletes its caches, including the prebuilt ones in `search/` beside its `.obr`.

Bump `obs_search::INDEX_VERSION` whenever the documents, the fields, the tokenizer or the normalizer change. A compile-time pin on `NORMALIZER_VERSION` is a reminder. After a bump, every cache is stale and is rebuilt in the background, while the old caches keep answering searches.

| What | Where | Override |
|---|---|---|
| Search index caches | `~/.cache/OpenBible/search` (Linux, `$XDG_CACHE_HOME`), `~/Library/Caches/OpenBible/search` (macOS), `%LOCALAPPDATA%\OpenBible\cache\search` (Windows) | `OBS_SEARCH_CACHE` |
| Prebuilt Starter caches | `search/` beside the Starter `.obr` files | — |

## Measuring

```sh
cargo build --release -p obs-cli
target/release/obs-cli index ~/.local/share/OpenBible/library/*.obr --out /tmp/obs-search
target/release/obs-cli search ~/.local/share/OpenBible/library/kjv-strongs.obr --cache /tmp/obs-search
target/release/obs-cli search ~/.local/share/OpenBible/library/kjv-strongs.obr '"the son of man"' --cache /tmp/obs-search
cargo test --release -p obs-store --test kjv_search_timing -- --ignored --nocapture
```

`obs-cli search` with no query runs the timing set for the whole Bible. It exits with a non-zero status if a median misses its target: 100 ms for words and Strong's numbers, 250 ms for phrases. Each run opens the cache, compiles the query and collects every hit, which is what `Session::search` does.
