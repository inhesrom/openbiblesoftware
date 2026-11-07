# Timeline events ride on a People & places index, named by TIPNR's ids

The Timeline (#185) needs dated records — periods, lives and events — that name people and places and say which Passages tell them, offline. Theographic Bible Metadata (CC BY-SA 4.0) has 450 dated events with their Verses and people, and birth and death years for 45 people whose both years its chronology fixes. We decided three things that are hard to undo once Starter libraries carry them:

1. **The Events dataset is part of a People & places index, not a seventh Resource type.** A People & places index may carry `Resource::events`: each an **Era**, a **Lifespan** or an **Event** (`obs_model::Event`) with a start and an optional end (`HistoricalDate`: an astronomical year, with month and day only where the source gives them), an `approximate` flag, the event it is part of, its people and places (`EventEntity`: an optional People & places id and a name) and its Passages. In the `.obr` they are the optional `events` and `event_passages` tables (older files read none, like `entity_locations`). Theographic ships in the Starter library as `theographic`, an index with Events and no entities of its own.
2. **People and places are named by TIPNR's ids.** The Starter build matches each Theographic person to the TIPNR person with one of their names who shares the most Verses with them, and each place to the TIPNR place of its name that shares the most Verses with the events at it (or the only one of that name). A Lifespan or a person in an Event then opens the same Factbook as the Passage Guide; one without a match keeps its name only.
3. **Eras and Lifespans are derived at build time, from the same chronology.** Eras are a short, reviewed list in the converter (`obs_formats::ERAS`), each bounded by the start or end of a named Theographic event (the Exile: from the end of Zedekiah's reign to seventy years after Daniel's deportation), so the bands agree with the events inside them and a dataset update that loses an anchor fails the build. A person with both years in `People.csv` gets a Lifespan from them; anyone else with a `Lifetime of …` event gets that event as their Lifespan.

## Considered Options

- **A seventh Resource type ("Timeline", "Chronology"):** says what it is, but every place that lists Resource types gains a case, an older app could not open the `.obr`, and ADR 0009 and 0010 already chose to keep datasets that tools read within the existing types. The events identify happenings and list the Passages that tell them, as a People & places index identifies people and places.
- **Events as entities of a new kind with mentions:** reuses the entity tables, but entities have no dates, the Passage Guide and Factbook directory would list events as people or places, and a mention cannot say "from 1055 to 1015 BC".
- **Eras from the data alone:** Theographic has no Patriarchs or Exile records (only kingdoms and reigns). Hand-dated eras could drift from the chronology the events use; anchoring each bound to an event keeps them in one chronology.
- **Theographic's own people and places as entities:** would list David twice in the Passage Guide and give him two Factbooks. TIPNR's records are already the library's.
- **The Open catalogue instead of the Starter library:** the ticket allowed either. The Starter library has TIPNR beside it at build time to share ids, and the Timeline then works from first launch. The Resource is 0.3 MB.

## Consequences

- One chronology: Floyd Nolen Jones's for the Old Testament and the Gospels, Todd Atwood's for Acts, as Theographic gives them. The Tool says whose dates they are ("Dates as Theographic gives them") and marks the six dates the source flags approximate with "c."; other chronologies differ by decades for the patriarchs and the kings.
- The Resource is CC BY-SA 4.0 (Free to share); its share-alike covers the `.obr`, which ships as a separate file under that licence with Theographic's credit and our change record.
- `People.csv` carries Easton's dictionary text, which Theographic took from a CCEL edition: the converter never reads that column (the CCEL rule, #20).
- TIPNR names Jacob "Israel", so his Lifespan reads "Israel", as his Factbook does.
- An imported People & places index with Events works in the Timeline as it is; the empty state offers only the indexes in `timeline::EVENT_INDEXES`.
