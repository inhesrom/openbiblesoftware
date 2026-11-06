# The Search Tool: Bible mode (#59), its Results views (#146) and Library mode (#60)

The Search Tool is one Tab with a **Bible / Library / Notes** switch (#16 rule 9). Bible and
Library modes are described here; Notes mode (#64) is described in [notes-tool.md](notes-tool.md).
Bible and Library modes share the query grammar and parser, the highlighting, the recent queries
(kept for each mode), the keyboard navigation, the background jobs and the rules for opening
hits. The Search index caches they read are described in [search-index.md](search-index.md).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `Highlighter` | Hit highlighting in Rust (#16 rule 7). Surface terms are found by re-tokenizing the display text with the shared normalizer. `strong:`/`lemma:` terms mark the matching `tagged_word` spans, and `@…` terms the parts whose Morphology meets them. In a Hebrew word of several morphemes, only the matching morphemes are marked. |
| `obs-search` | `morph` (`MorphCategory`, `MorphCriterion`, `MorphCriteria`) | Morphology criteria (#145): the plain-English values of each kind of grammar, the words a query may use for them, and whether a decoded code meets them. |
| `obs-engine` | `bible_search` | Per-Tab state (`Tab::search`): mode, query, Scope, chosen Bibles, inline error, results, selection. Also Scope parsing and presets, the mapping from Standard scope to verse ordinals through each Verse map, the Verse list in Standard canonical order, snippet pages, recent queries, and opening hits. Commands: `search.run`, `search.mode`, `search.scope`, `search.bible`, `search.open`. |
| `obs-engine` | `search_views` | Bible mode's Results views (#146): `ResultsView`, the Results filter (`ResultsFilter`, applied to a copy of the results), `HitGrid`, `chart_bars`, and the Analysis table (`SearchAnalysis`, made by a background job, sorted by `AnalysisSort`, written as CSV). Commands: `search.view`, `search.filter`, `search.chart`, `search.sort`, `search.export_csv` (and `search.open`'s `all`). |
| `obs-engine` | `library_search` | Library mode on the same Tab state and jobs: the Resource picker's scope (`LibraryScope`), grouping by Resource, excerpts, and opening a hit at its paragraph. Commands: `search.resource`, `search.resource_type`, `search.collection` (and `search.run`'s `resources`). |
| `obs-engine` | `Workspace::navigate_text`, `Effect::NavigateText` | A Resource Tab goes to a Key and scrolls a Text range to the top (a Library hit's paragraph). |
| `obs-ui-egui` | `shell/search_views.rs` | The Verses / Grid / Chart / Analysis switch, the filter chip, the Grid and Chart (egui), the Analysis table (its words, Lemmas and Context through the text pipeline) and the CSV save dialog. |
| `obs-ui-egui` | `shell/search_tab.rs`, `search_view.rs` | The Tab body: fields, pickers, the Morphology chooser, summary line, and the virtualized results pane drawn by the inline-snippet mode, with the highlights painted under the glyphs. Also ↑/↓/Enter and clicks. |
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

## Morphology criteria (#145)

In Bible mode a query can ask for grammar in plain English. `@` (or `morph:`) starts
**Morphology criteria** for one word, and takes the words after it while they name criteria:

| Query | Finds Verses with |
|---|---|
| `@aorist imperative` | a word that is an aorist imperative |
| `@verb qal perfect third masculine singular` | a Qal perfect 3ms verb |
| `strong:G26 @noun genitive`, `G26 @gen` | G26 as a genitive noun (the Strong's number is the same word's) |
| `lemma:ἀγαπάω @verb aorist`, `lemma:agapao @aorist` | ἀγαπάω in the aorist |
| `"love one another" @imperative`, `@imperative -@aorist` | the phrase and an imperative; an imperative but no aorist |
| `@genitive love` | a genitive and the word `love` (`love` names no criterion, so it ends them) |

- **Values.** Part of speech (noun, adjective, verb, pronoun, article, preposition,
  conjunction, adverb, particle, interjection, suffix); tense (present, imperfect, future,
  aorist, perfect, pluperfect, consecutive perfect, consecutive imperfect, conjunctive
  imperfect); voice (active, middle, passive, deponent); mood (indicative, subjunctive,
  optative, imperative, jussive, cohortative, infinitive, participle); case; number
  (singular, plural, dual); gender (masculine, feminine, neuter, common); person (first,
  second, third); Hebrew and Aramaic stem (Qal, Niphal, Piel … Peal, Pael …); state
  (absolute, construct, determined). A two-word value is written with `-` or as two words
  (`consecutive-imperfect`). Short forms: `1st`, `sg`, `pl`, `masc`, `gen`, `acc`, `adj`,
  `wayyiqtol`, `weqatal` and others. Case never matters.
- **Meaning.** Every criterion holds for the same morpheme part, read through the same
  decoder as Word Info. A Greek second aorist is an aorist; `middle` and `deponent` each
  match `middle deponent`; `participle` and `infinitive` are moods here, as in Robinson's
  codes. Two values of one kind (`@aorist present`) find nothing. A second `@` starts another
  word. A word that names no criterion directly after `@` is an error (`“aorst” is not
  Morphology …`), and Notes mode refuses `@` (Notes have no Tagged words).
- **Strong's number or Lemma.** A `strong:`, bare `G26` or `lemma:` term directly before the
  `@` belongs to that word: `strong:G26 @noun genitive` is one genitive G26, while
  `G26 love @genitive` is G26, `love` and some genitive word anywhere in the Verse. An
  explicit `AND` or a parenthesis between them keeps them apart too: `G26 AND @genitive`,
  `@genitive AND G26` and `(G26) @genitive` all mean G26 and some genitive word in the Verse.
- **Highlights.** The words (or, in Hebrew, the morphemes) that meet the criteria; the
  Analysis lists them with their Morphology in plain English.
- **Morphology chooser.** The **Morphology…** button beside the Bible / Library / Notes switch shows a menu for each kind
  of grammar (Any by default). The criteria chosen show as their query text. **Add to query**
  adds it at the end of the query box, so after `G3936` it describes that word; **Add and
  search** also runs the search. The chooser writes exactly the query typing would, so both
  give the same results.

With the real Starter TAGNT, `@aorist imperative` scoped to Romans finds 23 Verses, the same
ones as reading every Romans code for a Robinson aorist imperative (`V-[2]A?M`); most are the
greetings of Romans 16. The index answers in under 1 ms; `@verb` over the whole of TAGNT takes
15 ms and over TAHOT 34 ms, `strong:G26 @noun genitive` and `lemma:ἀγαπάω @verb aorist` 1–2 ms
(release build, cache open and compile included; #19 targets 100 ms for words and 250 ms for
phrases). `real_starter_tagnt_aorist_imperatives_in_romans` checks this.

Previews, from the real app and the Starter TAGNT: [`@aorist imperative` in Romans](../previews/search-morphology-romans-real.png)
and [the chooser adding `@verb aorist imperative` after `strong:G3936`](../previews/search-morphology-chooser-real.png)
(the app's `OBS_SCRIPT`, `OBS_INPUT` and `OBS_SHOT` automation). [The chooser over the test
excerpt](../previews/search-morphology-chooser-kittest.png) comes from
`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib morphology_chooser`.

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

## Results views (#146)

Above Bible mode's results a switch shows them four ways. The choice stays with the Tab across
searches.

- **Verses**: the Verse list.
- **Grid**: a row for each Bible book with a hit (canonical order) and a column for each
  searched Bible. A cell holds that Bible's matching Verses in the book, shaded by its count
  over the largest cell's count. Clicking a cell sets the **Results filter** to that book and
  Bible. Clicking a book name sets it to the book in every Bible.
- **Chart**: a bar for each Bible book with a hit, or (**By chapter of…**) a bar for each
  chapter of one book, empty chapters included. A bar counts rows of the Verse list, so a
  Verse matched in two Bibles counts once. Clicking a bar sets the filter to that book or
  chapter.
- **Analysis**: one row per matched word of every hit. Columns: Reference (the row's Standard
  reference), Bible, the word, its Lemma, its Morphology and a short Context. The word is
  the whole Tagged word(s) a highlight touches, so a Hebrew root highlighted without its
  prefix is still one word, and KJV+Strong's `the love` is the span it tags. Lemma and
  Morphology come from the parts the query matched (all parts for a plain word search). A
  part without a Lemma (KJV+Strong's) borrows the headword of the first Lexicon in Preferred
  order that has its number, as Word Info does. Morphology reads in plain English through the
  same decoder as Word Info (`noun, acc. fem. sing.`); the code as tagged (`N-ASF`) is the
  cell's tooltip, and a code the decoder cannot read stays as written. A background job makes
  the table the first time it is shown for a search, and again after a new search while it is
  shown. If a searched Bible has since been removed from the Library, or its file or tags
  cannot be read, the job fails with that message under the search box instead of making a
  table (and an export) without that Bible's hits. Rows open their Verse like a hit.
  - **Sorting**: click a header to sort by it, and again to reverse. Text compares
    case-insensitively and empty cells go last either way. Reference is canonical order;
    reversed, the Verses turn round but a Verse's words and Bibles keep canonical order.
    Context compares the words after the match, then before it, as a concordance does. Ties
    keep canonical order.
  - **Export CSV…** saves the table in the order shown: a header row, then six fields per
    row (the Context field is the whole hit text). It is UTF-8 with a byte-order mark and
    CRLF line ends (RFC 4180), so spreadsheets read Greek and Hebrew correctly. A field
    starting with `=`, `+`, `-` or `@` gets a leading `'`, so it is never read as a formula.

The **Results filter** applies to a copy of the results, held in `SearchTab::results`; the
whole set stays in `SearchTab::unfiltered`, which the Grid, Chart and Analysis read. Setting
the filter switches to Verses. The filter shows as an `Only 1 John · KJV ×` chip by the
switch. A new search, or the chip's ×, clears it. Snippet pages are fetched again for the
filtered rows. The summary line counts the filtered rows in Verses and every result in the
other views.

With the real Starter KJV+Strong's and TAGNT, `strong:G26` (ἀγάπη, the noun) finds 106
Verses in each. 1 John stands out with 14, then 1 Corinthians (11) and Ephesians (10); John
has 6. John and 1 John stand out together for the verb, `strong:G25` (John 27, 1 John 17).
`real_starter_g26_grid_and_analysis` checks both.

Previews: [Grid](../previews/search-grid-g26-real.png), [Chart](../previews/search-chart-g26-real.png),
[a Grid cell's Verses](../previews/search-grid-filtered-real.png) and
[Analysis by Morphology](../previews/search-analysis-g26-real.png), from the real app and the
Starter library. The `*-kittest.png` previews come from the open test excerpts, made with
`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib search::`.

## Library mode (#60)

- **Search scope (#16 rule 10).** The Resource picker chooses whole Resource types
  (Commentaries, Lexicons, Dictionaries), Collections ([collections.md](collections.md)) and
  single Resources. The default is every
  Commentary, Lexicon and Dictionary. A chosen type also covers Resources added to the library
  later. The chips under the query box show what is searched (× leaves one out), and
  **Choose…** lists each type with its Resources as checkboxes. Leaving out one Resource of a
  whole type or Collection drops every part of the scope that
  covered it and keeps the others those covered, one by one. Choosing every Resource of a type
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
`resources` (Library mode: comma-separated `commentary`, `lexicon`, `dictionary`,
`collection:<name>` and resource ids), `mode`, `tab` and `split`. It runs in the most recently active Search Tab, or opens one.
Without `mode` the search runs in whatever mode that Tab was left in, so a hand-over always
passes the `mode` it means (`"bible"` for every hand-over below).

- Word Info's **Search this word** runs `strong:<the word's number>` with `mode: "bible"`
  in the Bible the word was read in.
- Word Study's Occurrences **Show all in Search** (#56) runs `strong:G5485` with `mode:
  "bible"` in the preview Bible and the counted Bible (`bibles: "kjv-strongs,tagnt"`), with
  the number counted; the preview Bible is left out when it tags only the base of an
  extended number. A
  **Translated as** rendering (#56) runs `strong:G5485 "favour"` with `mode: "bible"` in the
  Bible the renderings come from.

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
