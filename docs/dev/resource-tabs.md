# Commentary, Lexicon and Dictionary Tabs (#49)

Resource Tabs share `ReadingDoc`, `ReadingView` and `reading_pane`: structural spans,
layout workers, glyph atlas, selection, Page tone and AccessKit stay on the existing
Resource-text path. No Resource prose is rendered with egui labels.

## Navigation contract

- `ResourceNavigation` in the engine indexes a Resource's Keys. `ReadingDoc` loads
  those Keys from `.obr` and shares the index with the Tab. This mirrors the existing
  Bible Verse-map hand-off when a Resource opens.
- Commentary boxes parse Standard references, including contextual chapter/Verse
  input. Link sets translate a Bible's Verse map first, then choose the last section
  whose Passage starts at or before that Standard Verse. The follower records its
  section start; it never sends that fallback back to the Bible.
- Lexicon boxes accept Strong's numbers, original-script Lemmas and the shared
  transliteration scheme. Extended Strong's numbers try exact, then base. Dictionary
  boxes accept normalized headwords and aliases.
- `tab.go {tab, text}` handles all three; `tab.previous_entry` / `tab.next_entry`
  move between entries, and `tab.back` / `tab.forward` retain per-Tab history.
  Compatible entry-Key schemes follow Link sets; Passage-only members do not move.
- `entry_location` is an optional, backwards-compatible field in `TabSnapshot`.
  Last session and named Layouts restore it without replaying navigation history.
- Callers opening an entry can use `resource.open`, apply its Opened effect (loading
  the Resource), then `tab.go`. No dependency on the Library Tool or Word Info UI.

## Decisions

- "Nearest earlier" is ordered across Standard chapters and Bible books, not by
  serialized unit id. At coincident starts, the narrowest Passage wins, then stable
  KeyEntry order. Before the first section, explicit navigation reports no earlier
  section and leaves the current view unchanged; Link-set following does not invent
  a later target.
- Entry navigation has one stop per primary entry unit, in Key order. Lemma and
  headword aliases do not duplicate stops. Exact primary Keys beat aliases; ambiguous
  normalized/transliterated Keys choose the first stable KeyEntry. No chooser is
  introduced in this ticket.
- Tests use the repository's existing real-event egui shell harness instead of
  introducing a second kittest dependency. Rendered snapshots and the recording
  come from that same harness. The recording is a sequence of real rendered shell
  frames, held for two seconds per navigation step, not a desktop capture.

## Evidence

- Engine `resource_navigation_tests`: nearest section (exact, contained, gap,
  chapter/book boundary, coincident starts, before-first and empty); Standard
  reference parsing and no feedback; Strong's fallback; normalization,
  transliteration and aliases; entry order, Link sets, history and saved Keys.
- UI `resource_tabs_tests`: typing G26 in the actual box; pipeline/AccessKit entry
  text; linked Commentary including Hebrew-numbered Bible translation; header
  navigation; Dictionary alias; Workspace restore; persisted `.obr` Keys; narrowed
  Key byte ranges.
- [Commentary snapshot](../previews/resource-tabs-commentary.png)
- [G26 snapshot](../previews/resource-tabs-g26.png)
- [Navigation recording](../previews/resource-tabs.mp4)

The small synthetic test Resources are not quoted Starter editions. Source pins,
Attribution and the Starter library are unchanged.

Commands run through the shared `cargo-slot` semaphore:

```sh
cargo test -p obs-engine resource_navigation
cargo test -p obs-ui-egui resource_tabs
cargo xtask ci
cargo xtask bench
UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui resource_tabs_previews
```

All local gates passed, including wasm (the target was available in this run).
The benchmark met all thresholds: 0% missed frames, origin settles in 0 frames,
all four panes in 1 frame, text GPU 1.0 MiB.

Preview test writes `target/previews/resource-tabs/*.ppm`. PNG snapshots are those
frames encoded with ffmpeg; the MP4 uses `frame-%02d.ppm` at one frame per two seconds,
H.264/yuv420p, 30 fps output.
