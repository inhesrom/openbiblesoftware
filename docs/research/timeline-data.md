# Timeline data: dated events, lives and eras (#185)

Checked live on 2026-11-06 from this host. Decisions: [ADR 0011](../adr/0011-timeline-events-ride-on-a-people-and-places-index.md).

## Theographic Bible Metadata

- **Source:** <https://github.com/robertrouse/theographic-bible-metadata>, latest commit `3f461f2f3cf8939f2b79430ae238a36528024ed3` (2026-11-02, the monthly Airtable export). The repository has `json/`, `CSV/`, `geo/`, `neo4j/` and a GraphQL API over the same tables.
- **Licence:** `readme.md`: "This work is free to use and copy under a Creative Commons Attribution-ShareAlike 4.0 International License. Please also credit the original sources, several of which have their own licenses." `LICENSE` is the CC BY-SA 4.0 legal code (sha256 `7abe19ec…6f297`); the GitHub API reports `cc-by-sa-4.0`. CC BY-SA is Free to share (CONTEXT.md, Licence class). Archived in `starter/evidence/theographic-*`.
- **Its sources** (`docs/data-source-bibliography.md`): event dates and people's birth and death years follow Floyd Nolen Jones, *The Chronology of the Old Testament* (Master Books; free PDF from the author's ministry); Acts follows timelines Todd Atwood, ThM, provided to the project; people were identified with TIPNR (STEPBible, CC BY 4.0) and two genealogy sites. These are dates and identifications compiled into Theographic's dataset; no text of those works is in what we read. The attribution credits Jones and Atwood as the README asks.
- **CCEL:** Theographic's Easton's Bible Dictionary text was "parsed from the Christian Classics Ethereal Library XML edition". It sits in `People.csv`'s `dictText` (and the `easton` table). We download `People.csv` from GitHub (not a CCEL host) but never read that column; nothing from it is shipped. The `easton` table is not downloaded.

### What it gives

- **Events** (`CSV/Events.csv`, 246 KB, 450 rows): title, `startDate` (ISO 8601 astronomical years such as `-1011` for 1012 BC, or a calendar date `0030-04-04` where the text fixes a day: 50 events), `duration` (`7D`, `40Y`, `3M10D`, `2.5Y`, `1W`), `partOf` (parent event by title), `predecessor`/`lag` (how the dates were derived), `verses` (OSIS, KJV numbering = our Standard versification), `participants` and `locations` (lookup ids such as `david_994`), `groups`, `notes` (6 rows).
- **Approximate dates:** `rangeFlag` marks 6 events whose year is an approximation (The Fall, Tower of Babel, Job's Trial, Ten Commandments Given, Birth of Saul, David Kills Goliath). The CSV export leaves the column out, so we also pin `json/events.json` (1.8 MB) and read only `eventID` and `rangeFlag` from it.
- **People** (`CSV/People.csv`, 1.6 MB, 3,067 rows): `birthYear` for 70 and `deathYear` for 61 (both for 45), from Jones's Appendix F; names, other names, gender and Verses. David: `-1084` to `-1014` (1085–1015 BC).
- **No Eras.** It has the kingdoms (`United Kingdom`, `Northern Kingdom (Israel)`, `Southern Kingdom (Judah)`) and reigns, but no Patriarchs or Exile records, so the converter dates eleven Eras from its events (`obs_formats::ERAS`).

### Converted (full sources)

`OBS_TEST_THEOGRAPHIC=<dir> OBS_TEST_TIPNR=<file> cargo test -p obs-formats --test theographic full_sources -- --nocapture`:

- 480 records: 11 Eras, 46 Lifespans (45 from birth and death years, plus Joshua from his `Lifetime of` event; 26 `Lifetime of` events folded into Lifespans), 423 Events; 6 approximate.
- People named by TIPNR's ids: 870 of 900 mentions. Unmatched, so shown by name only: Holy Spirit, Satan, Candace, Micah, Sergius, Herod Agrippa I, Herod Antipas, Lot's two daughters, the mother of Zebedee's children and the mother of Simon's wife (TIPNR names, spells or splits them differently). Places (each matched once, by the Verses of every event at it): 465 of 473; unmatched: Adria, Appii Forum, Gennesaret, Phenicia, Plain of Jordan, Solomons Portico.
- Eras (Jones's dates): Primeval history 4004–1996 BC, Patriarchs 1996–1635 BC, Egypt and the Exodus 1635–1451 BC, Conquest and Judges 1451–1095 BC, United Kingdom 1095–975 BC, Divided Kingdom 975–586 BC, Exile 586–536 BC, Return and Restoration 536–441 BC, Between the Testaments 441–4 BC, Life of Jesus 4 BC–7 April AD 30, Early Church 7 April AD 30–AD 60 (each bound keeps its anchor's day; an Era ends on the date the next begins).
- David: Lifespan 1085–1015 BC under `David@Rut.4.17-Rev`; Reign of David 1055–1015 BC. 1 Kings 6: Construction of Solomon's Temple (1012–1005 BC, told in 1 Kings 5:1–7:51) and Reign of Solomon (1015–975 BC).
- Built Resource: 0.3 MB (`theographic.obr`), plus a 37 KB Search index cache; the Starter budget stays under 400 MB.

## STEPBible TIPNR

TIPNR (already in the Starter library) identifies people and places with every reference, but carries no dates beyond descriptive text ("living at the time of United Monarchy"), so it cannot place anything on an axis. It is what the Timeline names people and places by.
