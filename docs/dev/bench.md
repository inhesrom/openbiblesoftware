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
| `find` | Find next (#175) through matches far apart | ≤ 1% missed, each step ≤ 1 frame |

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

Every feature scenario has the steady-scrolling budget: at most 1% missed frames. A settle
budget counts frames from an action until the feature shows its result (0: its own frame). A
synchronous action is a jump's origin (≤ 1 frame); work read from the library on a worker has
the #19 search budget, 100 ms (6 frames at 60 Hz). A feature scenario that cannot be set up,
or one listed in `bench.toml` but missing from the report, fails the gate.

Every feature scenario also has the #19 synchronous-work budget: at most 2 ms a frame (p99).
Work that fits in a frame but takes longer than that still fails the gate, though no frame is
missed at 60 Hz.

- `interlinear` and `find` measure the reading panes' UI-thread layout, as the reading
  scenarios do.
- The Shell-driven scenarios measure the feature Tab's own drawing (its body, egui text
  layout included). Every timed frame must draw it. When an action's result is made off the
  UI thread or drawn afresh (the Atlas following a Link set, a Search view switched, a Guide
  moved), the frames from the action until its result shows are exempt, as #19 exempts the
  block a jump lands in. Those frames are held to the settle and missed-frame budgets
  instead. Get Resources filters and sorts its catalog on the UI thread in the frame a search
  key is typed or the type filter changes; those frames are not exempt.

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
