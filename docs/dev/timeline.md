# Timeline (#185)

A Tool that shows biblical history on a zoomable horizontal axis, offline. Decisions: [ADR 0011](../adr/0011-timeline-events-ride-on-a-people-and-places-index.md). Data source and licence: [research/timeline-data.md](../research/timeline-data.md).

## Contract

- **Tab.** `TabKind::Tool(Tool::Timeline)`, `tool.timeline {passage?, new_tab?, panel?, split?}` (Tools menu and launcher, the Panel `+`). Key schemes: Passage, so it joins Link sets and follows them like the Atlas. Without a Passage it shows the whole Timeline.
- **Data.** `timeline::read_timeline(library)` (a worker job, cached for the library in `References::timeline`): every `Event` of every People & places index that has some, as `TimelineEntry`s (Eras, then Lifespans, then Events, each by date). `from`/`to` are positions in years (`timeline::position`: astronomical year plus month and day); `dates` is the label.
- **Dates.** `timeline::date_label`: `1012 BC`, `1055–1015 BC`, `4 BC–AD 30`, `31 March–7 April AD 30`, and `c. …` when the source marks the date approximate (`Event::approximate`, Theographic's `rangeFlag`).
- **Emphasis.** `Timeline::emphasis(passage)`: Events whose Passages overlap it; the focus is the one with the largest share of its own Verses in the Passage (ties: the shorter); Lifespans of the people those Events name (same TIPNR id, or same name when neither has one), and the Era the focus starts in. A single Verse no Event tells widens to its chapter (`Emphasis::widened`).
- **View.** `TimelineView` (centre year, points per year): `fit`, `around` (an entry with at least `FOCUS_YEARS` = 160 years around it), `pan`, `zoom_about` (keeps the year under the pointer; never further out than the whole Timeline), `ticks` (round BC and AD years, steps 1 to 1000).
- **Drawing** (`shell/timeline_tab.rs`): ticks; Eras as a band of names that also shades the axis; up to three rows of Lifespans, then rows of Events. Each entry goes in the first row where its bar (or diamond, for one day or year) and its label fit, emphasised ones first so they keep their labels; a label sits inside a wide bar, else after it, else before it. Emphasised entries are drawn in full colour, the focus ringed in gilt. Drag pans, the wheel or a pinch zooms, horizontal scroll pans; `−` `+` `Fit` `Passage` buttons.
- **Clicks.** An Event: `State::reference_navigate` to its first Passage (the Link set moves there, or the ★ Bible opens; Ctrl/Cmd: a new Tab). A Lifespan: `guide.factbook {entity}`. An Era: the view fits it. The list under the axis repeats the Passage's Events with their dates, Passage links and the people they name (each opens its Factbook).
- **Empty.** With no index that has Events: `Session::timeline_empty_state` offers the Starter library's `theographic` (`timeline::EVENT_INDEXES`).

## Data

- **Starter library:** lock entry `theographic` (importer `theographic-events`): `CSV/Events.csv`, `json/events.json` (for `rangeFlag` only) and `CSV/People.csv` at commit `3f461f2`, plus the TIPNR file (same pin as `tipnr`) for matching. Converter: `obs_formats::import_theographic_events`. 480 records: 11 Eras, 46 Lifespans, 423 Events, 6 approximate; 870 of 900 people and 465 of 473 places named by TIPNR's ids; 0.3 MB.
- **Model and store:** `obs_model::{Event, EventKind, HistoricalDate, EventEntity}`, `Resource::events` (People & places only, in the content hash); `.obr` tables `events` and `event_passages` (optional), `ObrReader::events`.

## Evidence

- Formats `tests/theographic.rs`: the Eras' dates, David's Lifespan and reign, the temple in 1 Kings 5-7, Lifetime events becoming Lifespans, calendar dates, TIPNR matching, determinism; full sources with `OBS_TEST_THEOGRAPHIC` and `OBS_TEST_TIPNR`.
- Engine `timeline_tests`: the two acceptance checks (`the_timeline_shows_the_patriarchs_kingdom_and_exile_eras_with_davids_life`, `opening_1_kings_6_emphasises_the_temples_construction`), widening, date labels, the view, the Tool's Tab, the empty library.
- UI `shell::tests::timeline`: 1 Kings 6 on screen with clicks to David's Factbook and the temple's Passage; the Patriarchs, United Kingdom and Exile Eras where their Passages are; the whole Timeline and the empty state.
- Real Starter library (manual): `OBS_LIBRARY=dist/starter cargo test -p obs-engine --lib starter_library_timeline -- --ignored --nocapture`.
