# The Search Tool: Bible mode (#59)

The Search Tool is one Tab with a **Bible / Library / Notes** switch (#16 rule 9). Bible mode is
built here. Library mode comes with #60 and Notes mode with #64; until then those modes show
"… arrives with #60/#64". The Search index caches it reads are described in
[search-index.md](search-index.md).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `Highlighter` | Hit highlighting in Rust (#16 rule 7). Surface terms are found by re-tokenizing the display text with the shared normalizer. `strong:`/`lemma:` terms mark the matching `tagged_word` spans. In a Hebrew word of several morphemes, only the matching morphemes are marked. |
| `obs-engine` | `bible_search` | Per-Tab state (`Tab::search`): mode, query, Scope, chosen Bibles, inline error, results, selection. Also Scope parsing and presets, the mapping from Standard scope to verse ordinals through each Verse map, the Verse list in Standard canonical order, snippet pages, recent queries, and opening hits. Commands: `search.run`, `search.mode`, `search.scope`, `search.bible`, `search.open`. |
| `obs-ui-egui` | `shell/search_tab.rs`, `search_view.rs` | The Tab body: fields, picker, summary line, and the virtualized results pane drawn by the inline-snippet mode, with the highlights painted under the glyphs. Also ↑/↓/Enter and clicks. |

## A search, step by step

1. **Enter** in the query box (or the menu bar's Search field, or a hand-over from another
   Tool) runs `search.run`. The query is parsed and compiled on the UI thread so errors show
   at once, inline under the box. The Scope is read the same way. The query goes into the
   recent list for its mode, which is stored in the User database under
   `search.recent.<mode>`.
2. For each chosen Bible (by default the ★ Bible, first in Preferred order) the engine opens
   its Ready index, or the stale one while it is rebuilt. Bibles still indexing are listed
   as "Indexing …" under the summary.
3. A background job then compiles the query, resolving `lemma:` transliterations through the
   Lexicons. For each Bible it takes the **outline** (verse list plus Verse map, cached for
   each content hash; see *Outline read-ahead* below) and turns the Standard scope into
   **verse-ordinal ranges**. A Bible
   Verse is in scope when its Standard Passage overlaps the scope's Passage list. Unmapped
   verses have no Standard Passage, so they are in scope only for Whole Bible, which searches
   every document. The job then searches each index in document order.
4. Hits become the **Verse list**: one row per Standard Verse, holding each Bible that
   matched there. An Unmapped verse gets a row of its own at its own number. The job also
   reads the first page of snippets (`PAGE_ROWS` rows). Each snippet is the hit's text from
   its `.obr`, with headings left out and white space collapsed, plus its highlight ranges.
5. The pane lays out only the rows in view, on the layout workers, and asks the engine for
   their snippet pages as the user scrolls.

## Outline read-ahead

Reading a Bible's outline takes about 0.4 s for a full Bible, so it is not left to the first
search. As soon as a Bible's index is Ready (found at startup, or built), or searchable
through a stale cache, its outline is queued to be read in the background. These reads run
one at a time, the ★ Bible (best in Preferred order) first. A shown Search Tab moves its
chosen Bibles to the front. An outline that cannot be read is remembered and not tried
again until the library changes; a search over that Bible then reports the error.

## Opening a hit (#16 rule 14)

- **Click or Enter.** If the Search Tab has a Link set, the set navigates to the Standard
  Verse, and each member translates it through its own Verse map. Otherwise the most recently
  active Tab of that Bible goes to the Verse in the Bible's own numbering. If there is no such
  Tab, a new Tab opens there. An Unmapped verse never moves a Link set.
- **Ctrl/Cmd-click** (or Ctrl/Cmd+Enter) always opens a new Tab, split off to the right.

## Hand-over from other Tools

`search.run` takes `text`, plus optional `scope`, `bibles` (comma-separated resource ids),
`mode`, `tab` and `split`. It runs in the most recently active Search Tab, or opens one.
Without `mode` the search runs in whatever mode that Tab was left in, so a hand-over always
passes the `mode` it means (`"bible"` for every hand-over below).

- Word Info's **Search this word** runs `strong:<the word's number>` with `mode: "bible"`
  in the Bible the word was read in.
- Word Study's **Show all** (#55) should run `strong:G5485` with `mode: "bible"`. Its
  **Translated as** filter (#56) should run `strong:G5485 "favour"` with `mode: "bible"`.

## Measuring

```sh
cargo test --release -p obs-engine --lib real_starter -- --ignored --nocapture
cargo test --release -p obs-ui-egui --lib real_library_search_timing -- --ignored --nocapture
```

Both tests need the Starter library in the platform data folder. They build the Search index
caches once into `$TMPDIR/obs-real-search-cache`. The second test prints the search time and
how long the first results page takes to draw after the results arrive (#19 target:
≤ 150 ms).
