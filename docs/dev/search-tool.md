# The Search Tool: Bible mode (#59) and Library mode (#60)

The Search Tool is one Tab with a **Bible / Library / Notes** switch (#16 rule 9). Bible and
Library modes are built; Notes mode comes with #64 and until then shows "Notes search arrives
with #64". Both modes share the query grammar and parser, the highlighting, the recent queries
(kept for each mode), the keyboard navigation, the background jobs and the rules for opening
hits. The Search index caches they read are described in [search-index.md](search-index.md).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `Highlighter` | Hit highlighting in Rust (#16 rule 7). Surface terms are found by re-tokenizing the display text with the shared normalizer. `strong:`/`lemma:` terms mark the matching `tagged_word` spans. In a Hebrew word of several morphemes, only the matching morphemes are marked. |
| `obs-engine` | `bible_search` | Per-Tab state (`Tab::search`): mode, query, Scope, chosen Bibles, inline error, results, selection. Also Scope parsing and presets, the mapping from Standard scope to verse ordinals through each Verse map, the Verse list in Standard canonical order, snippet pages, recent queries, and opening hits. Commands: `search.run`, `search.mode`, `search.scope`, `search.bible`, `search.open`. |
| `obs-engine` | `library_search` | Library mode on the same Tab state and jobs: the Resource picker's scope (`LibraryScope`), grouping by Resource, excerpts, and opening a hit at its paragraph. Commands: `search.resource`, `search.resource_type` (and `search.run`'s `resources`). |
| `obs-engine` | `Workspace::navigate_text`, `Effect::NavigateText` | A Resource Tab goes to a Key and scrolls a Text range to the top (a Library hit's paragraph). |
| `obs-ui-egui` | `shell/search_tab.rs`, `search_view.rs` | The Tab body: fields, pickers, summary line, and the virtualized results pane drawn by the inline-snippet mode, with the highlights painted under the glyphs. Also ↑/↓/Enter and clicks. |
| `obs-ui-egui` | `ReadingView::jump_to_text`, `pane::Flash::Text` | Scrolling a Resource Tab to a paragraph and flashing it. |

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

## Library mode (#60)

- **Search scope (#16 rule 10).** The Resource picker chooses whole Resource types
  (Commentaries, Lexicons, Dictionaries) and single Resources. The default is every
  Commentary, Lexicon and Dictionary. A chosen type also covers Resources added to the library
  later. The chips under the query box show what is searched (× leaves one out), and
  **Choose…** lists each type with its Resources as checkboxes. Leaving out one Resource of a
  whole type keeps the type's other Resources, one by one. Choosing every Resource of a type
  one by one makes it the whole type again. Bibles are searched in Bible mode; Cross-reference
  sets and People & places indexes have no prose to search.
- **Results (#16 rule 12).** Groups by Resource: Commentaries, then Lexicons, then
  Dictionaries, each type in Preferred order. Resources without hits are left out. Inside a
  group, hits are best first (`bm25`). The summary says
  `9 paragraphs in 4 Resources (4 searched)` and each group's header says
  `TCC — Test Covenant Commentary · 3 hits`. A row is the unit's title or Key (a Strong's
  number goes before a Lexicon's Lemma title: `H1285 · בְּרִית`), drawn by the text pipeline
  so Greek and Hebrew titles shape correctly, then an excerpt of the paragraph: about 320
  bytes around its first hit, cut at spaces, with `…` where text was left out.
- **Documents.** A row is one Search index document: a paragraph, or a heading or other text
  outside paragraphs (#58). That is why a Dictionary's `Covenant` heading can be a hit of its
  own (and, being short, its best one).
- **Opening a hit (#16 rule 14).** Click or Enter: a Tab of that Resource in the Search Tab's
  Link set, else the most recently active Tab of that Resource, else a new Tab. The Tab goes
  to the Key covering the paragraph, so Back returns to where it was and a Commentary's Link
  set follows its Passage. Then it scrolls the paragraph to the top of the view and flashes it.
  The paragraph stays at the top when the pane is resized, until the user scrolls.
  Ctrl/Cmd-click (or Ctrl/Cmd+Enter) always opens a new Tab, split off to the right.

Previews, from the synthetic open-text test Resources: [`covenant` results](../previews/search-library-results.png)
and [a hit opened at its paragraph](../previews/search-library-opened.png). To make them again, run
`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui library_search_previews`, which writes
`target/previews/search-library/*.ppm`.

## Hand-over from other Tools

`search.run` takes `text`, plus optional `scope`, `bibles` (comma-separated resource ids),
`resources` (Library mode: comma-separated `commentary`, `lexicon`, `dictionary` and resource
ids), `mode`, `tab` and `split`. It runs in the most recently active Search Tab, or opens one.
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

The first command also runs `real_starter_library_covenant_in_mhc_jfb_easton_and_smith`, which
searches the Starter MHC, JFB, Easton and Smith for `covenant`. On the development machine it
found 2324 paragraphs (MHC 1601, JFB 515, Easton 95, Smith 113) in 7 ms of searching, 17 ms
including the first page of excerpts.
