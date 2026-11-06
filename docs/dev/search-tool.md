# The Search Tool: Bible mode (#59), its Results views (#146), Library mode (#60), Proximity search and Word forms (#180)

The Search Tool is one Tab with a **Bible / Library / Notes** switch (#16 rule 9). Bible and
Library modes are described here; Notes mode (#64) is described in [notes-tool.md](notes-tool.md).
Bible and Library modes share the query grammar and parser, the highlighting, the recent queries
(kept for each mode), the keyboard navigation, the background jobs and the rules for opening
hits. The Search index caches they read are described in [search-index.md](search-index.md).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `Highlighter` | Hit highlighting in Rust (#16 rule 7). Surface terms are found by re-tokenizing the display text with the shared normalizer. `strong:`/`lemma:` terms mark the matching `tagged_word` spans, and `@…` terms the parts whose Morphology meets them. In a Hebrew word of several morphemes, only the matching morphemes are marked. |
| `obs-search` | `stem` | English stemming for Word forms (#180): the Porter stemmer behind an archaic layer (`-eth`, `-est`, `-edst`, `hath`, `doth`, `saith`). |
| `obs-search` | `morph` (`MorphCategory`, `MorphCriterion`, `MorphCriteria`) | Morphology criteria (#145): the plain-English values of each kind of grammar, the words a query may use for them, and whether a decoded code meets them. |
| `obs-engine` | `bible_search` | Per-Tab state (`Tab::search`): mode, query, Scope, chosen Bibles, inline error, results, selection. Also Scope parsing and presets, the mapping from Standard scope to verse ordinals through each Verse map, the Verse list in Standard canonical order, snippet pages, recent queries, and opening hits. The Word forms setting (`Session::search_word_forms`, User database preference `search.word_forms`). Commands: `search.run`, `search.mode`, `search.scope`, `search.bible`, `search.word_forms`, `search.open`. |
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
  explicit `AND`, a parenthesis or a `NOT`/`-` between them keeps them apart too:
  `G26 AND @genitive`, `@genitive AND G26`, `(G26) @genitive`, `G26 (@genitive)` and
  `G26 NOT NOT (@genitive)` all mean G26 and some genitive word in the Verse;
  `(G26 @genitive)` is still one genitive G26.
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

## Proximity search and Word forms (#180)

| Query | Finds |
|---|---|
| `faith NEAR/5 works`, `faith WITHIN 5 words works` | both in one Verse (paragraph) with at most 5 words between them, either order |
| `faith BEFORE/3 works` | the same, `faith` first |
| `"by faith" NEAR/3 work*`, `(faith OR hope) NEAR/2 love` | phrases, prefixes and alternatives on either side |
| `faith WITHIN 2 verses works` | each Verse with one side whose own Verse or a neighbouring one (same Bible book) has the other: a run of at most 2 Verses holds both |
| `(faith -dead) WITHIN 3 verses G2041` | any query on either side, in parentheses |
| `faith WITHIN 2 paragraphs works` | outside Bibles: neighbouring paragraphs of one text unit (`verses` and `paragraphs` both mean the Resource's own documents) |

- **Grammar.** The operators bind tighter than `AND` and `OR` (`grace faith NEAR/5 works` is
  `grace` and the pair) and are recognised only in these forms, in capitals like the other
  operators: `NEAR`, `near/5` and `faith WITHIN works` are words. A dangling operator is
  ignored. `NEAR/n` and `BEFORE/n` join words, phrases and groups of alternatives of them
  (`G26 NEAR/3 love` is an inline error); `WITHIN n verses` joins any two queries.
  `WITHIN 0 verses` means the same Verse, as `WITHIN 1 verse` does.
- **How it runs.** `NEAR/n` is an FTS5 `NEAR` group. `BEFORE/n` finds the `NEAR` candidates
  and keeps those whose word positions are in order. `WITHIN n verses` searches each side in
  the Search scope and keeps the Verses with a hit of the other side at most `n - 1`
  documents away in the same book, a second pass over the neighbouring Verse documents. The
  results stay a Verse list: James 2:21 ("works") is a row because 2:20 and 2:22 say
  "faith". Both sides are highlighted.
- **Notes mode** takes `NEAR/n`; `BEFORE/n` and `WITHIN n verses` are an inline error there.
- **Word forms.** The **Word forms** checkbox beside the query (Bible and Library modes; off
  by default) makes each bare English word match every word of its stem in each index:
  `love` → `love OR loved OR loves OR loving OR loveth OR lovest OR lovedst …`, whatever that
  Bible actually contains. The stemmer is Porter's (checked against his 23,531-word reference
  vocabulary) after an archaic layer: `-eth` and `-est` read as `-ing` (`loveth` → `loving`,
  `seeth` → `seeing`, `lieth` → `lies`), `-edst` as `-ed`, and `hath`, `hast`, `doth`, `dost`,
  `saith`, `didst`, `wouldst` and the like as their modern verbs; nouns that only end like
  that (`priest`, `forest`, `harvest`, `teeth`, `Nazareth`) and ordinals (`twentieth`) are left
  alone. Quoted words (even one), prefixes, Strong's numbers and Lemmas stay exact; Greek and
  Hebrew use `lemma:`. Highlights mark every word of the stem. Turning it on or off saves the
  setting (`search.word_forms`) and runs the Tab's search again.
- **No rebuild.** Each index finds the words of a stem in its own vocabulary when a stemmed
  query first runs, through an `fts5vocab` table in the connection's `temp` schema, looking
  only at the words that begin with the stem's first letters (`stem_prefix`). The cache
  format and `INDEX_VERSION` are unchanged, so no Search index is rebuilt for Word forms
  (see [search-index.md](search-index.md)).
- **Timings** on the real KJV (release build, cache open, compile and every hit included;
  `kjv_search_timing`): `~love` 0.8–1.7 ms (421 Verses; 281 without Word forms), `~the`
  14–25 ms, `faith NEAR/5 works` 0.4 ms, `faith BEFORE/3 works` 0.7 ms, `the BEFORE/3 lord`
  31 ms (6,078 Verses), `faith WITHIN 2 verses works` 1.3 ms, `the WITHIN 2 verses and`
  57 ms (27,261 Verses), `~love NEAR/3 ~god` 9–11 ms; #19's targets are 100 ms for words and
  250 ms for phrases. The same test checks that `faith WITHIN 2 verses works` finds James
  2:17, 2:18 and 2:21 and that `love` finds John 3:35 ("loveth") only with Word forms.

Previews, from the real app and the Starter KJV: [`faith WITHIN 2 verses works` in James 2](../previews/search-within-verses-james-app.png)
and [`love` in 1 John 4 with Word forms](../previews/search-word-forms-1-john-4-app.png) (the app's
`OBS_SCRIPT` and `OBS_SHOT` automation on macOS). [The toggle over the test
library](../previews/search-word-forms-kittest.png) comes from
`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib the_word_forms_toggle`.

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

## Saved searches and the Search history (#179)

`obs-engine`'s `saved_searches` keeps both, and `shell/search_sidebar.rs` draws them.

- **What is kept.** A `SavedSearch` (the type Favorites and Visits of a Search already keep):
  mode, query, the Scope field, the Bibles chosen (none: the ★ Bible), Library mode's Resource
  types, Collections and single Resources, and Notes mode's Notebook. A **Saved search** is one
  under a name (`NamedSearch`); names ignore case, and saving under a used name replaces it.
  The **Search history** (`SearchHistoryEntry`, with when it ran) gets every search that
  `Session::run_search` starts without an inline error, newest first. The same search run again
  moves to the top instead of being listed twice, and only the last `SEARCH_HISTORY_LIMIT`
  (50) are kept.
- **Storage.** The User database's `search.saved` and `search.history` preferences (JSON
  arrays), like the Collections and Copy presets, read once at `attach_user`. No schema
  change. A failed write is a message; the history is still kept for the session.
- **Running one.** `Session::open_search_in` (also what a Search Favorite or Visit runs) sets
  every field as kept on the Search Tab (the sidebar's own Tab, else the most recently active,
  else a new one) and searches. If the kept Search scope names a Bible or Resource no longer in
  the library, a Collection that was deleted or a Notebook that no longer exists, the search
  still runs over the rest and `SearchTab::notice` says what was left out ("Searched without
  what is gone: WEB is no longer in your library."). The next search clears it. With nothing
  left (a Library search of one deleted Collection) the usual inline error shows under it.
  The names of removed Resources come from the User database's remembered Resource names.
- **The sidebar.** On the right of the Search Tab, shown by itself when the Tab is at least
  640 px wide; **Saved & history** (beside the mode switch) hides or shows it. Saved searches
  by name, then History as `query · mode · scope`; each row's tooltip adds
  `Session::search_scope_details` (the Bibles, the Resource types, Collections and Resources,
  or the Notebook), so rows alike in their summary can be told apart. A click runs one in this
  Tab, Ctrl/Cmd-click in a new split; a Saved search's right-click menu deletes it; History's
  **Clear** asks first. **Save…** after Recent opens a name field (the query to start with)
  and saves the Tab's search with the query and Scope fields as typed, even when not yet run
  (Enter not pressed). Running a Saved search, history entry, Favorite or Visit bumps
  `SearchTab::restored`, which makes the UI show the fields as run over anything typed, even
  when the query and Scope are the same as before.
- **Command box.** A Saved search whose name or query contains the text (two characters or
  more) is a suggestion, rank 56, just above Favorites; it runs `search.open_saved`.
- **Home** (#178) is not built yet; it can list `Session::saved_searches()` and run
  `search.open_saved`.
- **Commands.** `search.save {tab, name, text?, scope?}` (`text`/`scope`: the fields as
  typed; default the last search's), `search.open_saved {name, tab?, panel?, split?}`,
  `search.delete_saved {name}`, `search.open_history {index, tab?, panel?, split?}` (0: the
  newest), `search.clear_history`, and `search.list_saved` (both lists as JSON, each with its
  `summary`). `search.open_*` answer the Tab, its inline error and its notice.

Tests: `crates/engine/src/saved_searches_tests.rs` (`strong:G26` scoped to the New Testament
saved as "agape in NT", run after a restart with the same results; 55 searches leave the last
50, each re-runs, Clear lasts across a restart; names; the Command box; a removed Bible and a
deleted Collection; saving typed fields; scope details) and
`crates/ui-egui/src/saved_searches_tests.rs` (Save… and the sidebar through real clicks and
typing, across a relaunch; Save… with fields typed but not run; a Saved search or history
row replacing what is typed). Previews:
[the sidebar after saving](../previews/search-saved-sidebar-kittest.png) and
[a Saved search whose Bible was removed](../previews/search-saved-notice-kittest.png), from
`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib saved_searches`.

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
