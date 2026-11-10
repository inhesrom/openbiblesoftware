# `cargo xtask bench`: the performance gate

`cargo xtask bench` runs `openbible --bench` in release mode and fails if any number is past
its threshold in `xtask/bench.toml`. CI runs it on every pull request (the `xtask bench` job),
then again with seeded slowdowns that must fail it (below). The budgets are the #19
performance targets.

## Scenarios

The reading scenarios drive 4 reading panes headlessly over a synthetic Torah (Genesis to
Deuteronomy, Tagged words), frame by frame at 60 Hz (`crates/ui-egui/src/bench.rs`):

| Scenario | What it does | Budget |
|---|---|---|
| `steady` | wheel scrolling | ≤ 1% missed frames, no placeholders |
| `fling` | flings up and down | ≤ 5% missed frames, no placeholders |
| `jumps` | Link-set jumps | origin settles in ≤ 1 frame, followers in ≤ 3 |
| `interlinear` | every pane with interlinear lines (#143), scrolling | ≤ 1% missed, no placeholders |
| `reading-layout` | every Reading layout option on in every pane (#209: one verse per line, drop numerals, justified and hyphenated, Narrow, no headings), scrolling | ≤ 1% missed, no placeholders |
| `find` | Find next (#175) through matches far apart | ≤ 1% missed, each step ≤ 1 frame |
| `resize` | every pane's width dragged a point a frame, to and fro (#255) | ≤ 1% missed |

The feature scenarios (#183) drive the whole Shell (`crates/ui-egui/src/bench/tools.rs`):
each writes a library of synthetic Resources sized like the real data
(`crates/ui-egui/src/bench/library.rs`) to a temporary folder, opens the feature with its
commands, pointer and keys through a headless egui context, waits (untimed) until it shows its
data, then times what a user does with it. A frame's time is `Shell::ui` plus egui's
tessellation, the UI thread's work before the GPU.

| Scenario | Data | What it does | Settle budget |
|---|---|---|---|
| `atlas` | 1,300 located places; 8–37 per chapter, 120 in each book's chapter 15 | pans and zooms the map, then the Bible moves chapter (the Atlas follows) | ≤ 6 frames |
| `search-grid` | three whole Bibles, `LORD` found in about 18,000 verses | switches to Grid and scrolls it | ≤ 1 frame |
| `search-chart` | the same search | Chart by book, then by chapter of Psalms, scrolled | ≤ 1 frame |
| `exegetical-guide` | a Tagged Hebrew Torah (10–30 words a verse) and its Lexicon | Genesis 24 word by word scrolled, then other chapters | ≤ 6 frames |
| `topic-guide` | two topical indexes of 1,000 topics; Prayer has 60 subtopics of 20 Passages each | scrolls, shows every subtopic, scrolls, switches topic | ≤ 6 frames |
| `get-resources` | a 3,000-row catalog (the live SWORD repositories list about 2,200) | types a search a key at a time, erases it, switches the type filter to Bibles and back, scrolls | ≤ 1 frame |
| `text-size` | two whole Bibles (verses of 15–35 words, one with Strong's numbers) and a whole-Bible Commentary, each in its own Tab at Psalm 119 | Ctrl+Plus and Ctrl+Minus in turn, then Ctrl+Plus held for 10 steps (a key repeat every other frame), the first Bible scrolled between | ≤ 6 frames and ≤ 100 ms |
| `text-size-tools` | the same two Bibles in a Text Comparison over Psalm 119 and a whole-Bible Search's Verses | the same presses, the Text Comparison scrolled between | ≤ 6 frames and ≤ 100 ms |
| `resource-text-size` | the first of those Bibles in two Tabs beside the second, at Psalm 119 | the first Bible's own Text size slider (#292) stepped up and down in turn, then dragged 10 steps a step a frame, the first Tab scrolled between | ≤ 6 frames and ≤ 100 ms |
| `search-preview` | the same two Bibles, the first the ★ Bible, their Search indexes built | types `LORD` (in nearly every Verse), `every man`, `said unto`, `shall be` into the Command box a key every 5 frames, Esc between | ≤ 9 frames and ≤ 150 ms |

Every feature scenario has the steady-scrolling budget: at most 1% missed frames. A settle
budget counts frames from an action until the feature shows its result (0: its own frame). A
synchronous action is a jump's origin (≤ 1 frame); work read from the library on a worker has
the #19 search budget, 100 ms (6 frames at 60 Hz). A feature scenario that cannot be set up,
or one listed in `bench.toml` but missing from the report, fails the gate.

Every feature scenario also has the #19 synchronous-work budget: at most 2 ms a frame (p99).
Work that fits in a frame but takes longer than that still fails the gate, though no frame is
missed at 60 Hz.

- `interlinear`, `reading-layout`, `find` and `resize` measure the reading panes' UI-thread
  layout, as the reading scenarios do. A `resize` frame's new width (every block's height
  estimated again, the block at the top laid out at once to keep the reader's place) is exempt
  as a jump is, and held to the missed-frame budget: before #255 it styled every block again,
  about 720 ms a frame.
- The Shell-driven scenarios measure the feature Tab's own drawing (its body, egui text
  layout included). Every timed frame must draw it. When an action's result is made off the
  UI thread or drawn afresh (the Atlas following a Link set, a Search view switched, a Guide
  moved), the frames from the action until its result shows are exempt, as #19 exempts the
  block a jump lands in. Those frames are held to the settle and missed-frame budgets
  instead. Get Resources filters and sorts its catalog on the UI thread in the frame a search
  key is typed or the type filter changes; those frames are not exempt.
- `text-size`, `text-size-tools` (#299) and `resource-text-size` (#292) time each Text size
  change in milliseconds too,
  from the key press's frame starting to the end of the first frame that draws every open
  Tab's text at the new size with no placeholder (`action_ms_max`, held to `max_action_ms`:
  100 ms). Held keys are timed from the last repeat: earlier steps' layouts must not queue up
  ahead of it. Before #299 a press styled every block of each whole-Bible Tab again on the UI
  thread, about 700 ms.
- `search-preview` (#295) times each query in milliseconds the same way, from its last key's
  frame starting to the end of the first frame drawing the Command box's Search preview hits
  for it: the 100 ms debounce, the search on a worker (`JobMode::default_for_host`, as the
  app runs it) and the frame showing it. The synchronous work timed is the Bible Tab under
  the dropdown's; the dropdown itself is held to the missed-frame budget with the whole
  Shell. About 120 ms on a 16-thread Ryzen (#295).

CI runs the gate three more times, each with a seeded slowdown that must fail it:

- `OBS_BENCH_SLOWDOWN_MS=3` adds 3 ms to every reading layout.
- `OBS_BENCH_FEATURE_SLOWDOWN_MS=3` adds 3 ms to every frame of each feature Tab, which still
  fits in a 60 Hz frame.
- `OBS_BENCH_ACTION_SLOWDOWN_MS=3` adds 3 ms to each feature Tab only in an action's own
  frame: in Get Resources, each frame its catalog query or type filter changes.

## Running it

- `cargo xtask bench`: the gate, as CI runs it.
- `cargo run --release -p openbible -- --bench --quick`: shorter, for a quick look;
  `--no-shell-scenarios` leaves out the feature scenarios, `--json OUT` writes the report.
- `cargo test -p obs-ui-egui -- every_shell_scenario`: every feature scenario sets up and
  shows its feature (unpaced, a few frames each).
- `cargo test -p obs-ui-egui -- get_resources_holds`: catalog filtering frames count toward
  the 2 ms budget.

## The scale scenario (#227)

`cargo xtask bench --scale` runs the app over a library of 2,000 Resources, as Get Resources
makes possible, and holds it to `[scale]` in `xtask/bench.toml`. It is a manual gate, not run
in CI: its library is about 22 GB of `.obr` files and Search index caches, more than a CI
runner's disk. Run it before a release, and after any change to the launch, the Library Tab,
Library search or the Passage Guide.

- `cargo xtask bench --scale [--dir DIR] [--quick]`: the gate. The library is generated under
  `DIR` (default `target/bench-scale`) the first time, in seconds, and kept for later runs
  (it is made again when the generator or the Search index format changes). On a filesystem
  that clones files (APFS, Btrfs, XFS) its copies share their data, so it takes about 4 GB of
  disk; elsewhere the full 22 GB. Delete the folder when done. `DIR` must be new, empty or
  a scale folder from an earlier run (it has a `stamp` file): any other folder is refused,
  never cleared.
- `cargo run --release -p openbible -- --bench --scale --dir DIR [--resources N] [--quick]
  [--json OUT]`: the scenario alone, with a smaller library for a quick look (the gate needs
  2,000).

The library (`crates/ui-egui/src/bench/scale.rs`) mixes kinds as the live SWORD repositories
do: 880 whole Bibles and New Testaments of about 25 words a verse (one in eight with Strong's
numbers), 120 Commentaries about the size of Jamieson-Fausset-Brown, 80 Dictionaries about
Easton's size and 40 Lexicons about Strong's size, in about 40 languages, plus a People & places
index. Fifteen templates are built once with their Search index caches, then copied under
2,000 identities (`obs_store::copy_obr_as`, `obs_store::copy_search_index_as`). Every app
folder (library, User database, caches, logs, Starter library) is inside `DIR`, so a run never
reads or writes the user's own.

The app is launched once to save the default layout as the Last session (a returning user),
then again for the measured run, from `main`:

| Measure | What it is | Budget |
|---|---|---|
| cold start | `main` to the first frame drawing the ★ Bible's text | ≤ 3 s (twice #19's 1.5 s) |
| Library filter | each key typed into the Library Tab's Filter box, to the frame showing the list | ≤ 50 ms |
| `scale library` | the Library Tab typed into, then scrolled | the feature budgets: ≤ 1% missed, sync ≤ 2 ms p99, ≤ 1 settle frame |
| peak RSS | the process's resident set, at its peak over the run, with 6 Panels open at the end | ≤ 500 MB (476.8 MiB) |

Get Resources (its catalog typed into and scrolled), a Library search for `lord` across every
Commentary, Dictionary and Lexicon, and a Passage Guide (Romans 8:28, then moved to 9:1) are
measured and reported, not gated: the regular gate holds their per-frame budgets. On Linux the
peak RSS is the kernel's; on macOS a thread samples the resident set (`ps`) every 250 ms.
Where the resident set cannot be read (Windows, which has neither), the peak is 0 and the
gate fails: memory not measured is not memory met.

### Results

On a Mac mini (Apple M6, 16 GB), 2,000 Resources and a People & places index, 60 Hz, before
and after the fixes made with the scenario (before: the same harness over the earlier code,
`--quick`; after: five runs, `--quick` and full, with other builds running on the machine):

| | before | after |
|---|---|---|
| cold start | 2,462 ms | 1,073–1,462 ms |
| Library filter, slowest key | 23 ms | 5.7–12 ms (median 0.9–2.4 ms) |
| Library Tab, synchronous work p99 | 6.0 ms (over 2 ms) | 0.45–1.0 ms |
| Library search `lord` (240 Resources, 192,434 hits) | 837 ms | 398–976 ms |
| Passage Guide opened / moved | 2,016 / 124 ms | 970–2,136 / 103–179 ms |
| RSS with 6 Panels (peak) | 792 MiB | 391–426 MiB |

The gate holds cold start to twice #19's floor-machine target (1.5 s). Of the launch, the
scale library itself costs about 0.4 s over one of 100 Resources: reading 2,000 manifests
(about 0.2 s) and checking 2,000 Search index caches (about 0.2 s). Doing either on several
threads made it about three times slower on macOS, so both stay on one.

The fixes:

- **Preferred order** (`obs_engine::Library`): each Resource's rank was a linear search of
  the saved order, and applying it re-inserted every Resource one by one, quadratic in the
  library's size; ranks are now a map and the order is applied in one pass. Recording the
  known Resource names at launch (`reanchor`) checks a set, not a list.
- **The Library Tab** lays out only the rows in view (every row keeps its place, for dragging
  and drop lines), instead of all 2,000 every frame.
- **The Passage Guide** keeps each Commentary's Passage Keys compact (`CommentarySections`:
  sorted, each unit id once), about a fifth of a reading Tab's navigation, so 120
  Commentaries' stay in memory.
- **Library search** moves each Resource's hits into the result rows instead of copying them,
  and closes each Search index once searched: a search finding 190,000 paragraphs no longer
  holds them twice or every index's page cache at once.
