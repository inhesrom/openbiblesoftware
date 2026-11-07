# Evidence: Theographic Bible Metadata, the Timeline's events (`theographic`)

- **Edition shipped:** the `robertrouse/theographic-bible-metadata` repository at commit `3f461f2f3cf8939f2b79430ae238a36528024ed3` (2026-11-02, "Monthly Airtable export"; the repository's latest commit when checked on 2026-11-06): `CSV/Events.csv` (450 events), `json/events.json` (the same table, read only for `rangeFlag`) and `CSV/People.csv` (3,067 people, read only for names, other names, gender, birth and death years and Verses).
- **Pinned files** (GitHub raw URLs at a commit, so their bytes cannot change):
  - <https://raw.githubusercontent.com/robertrouse/theographic-bible-metadata/3f461f2f3cf8939f2b79430ae238a36528024ed3/CSV/Events.csv>, **sha256** `c042e1413e26657f5a7d4c4e653ed8b44827995a8b3c7c384a05e762d01f6384` (246,197 bytes)
  - <https://raw.githubusercontent.com/robertrouse/theographic-bible-metadata/3f461f2f3cf8939f2b79430ae238a36528024ed3/json/events.json>, **sha256** `b0a84a4d310b3e81d2fbc43a86a84ee70b888b933c4e9b79c315c0405618e691` (1,778,003 bytes)
  - <https://raw.githubusercontent.com/robertrouse/theographic-bible-metadata/3f461f2f3cf8939f2b79430ae238a36528024ed3/CSV/People.csv>, **sha256** `60d88c59577bb9afbbf63a13d74a60161d186050d53ddeb4f24bdc8407a0d32f` (1,604,871 bytes)
  - TIPNR, read only to give people and places TIPNR's ids: the same file and pin as the `tipnr` entry, **sha256** `d595b9163d32a400bb0ef96b27a6de58335a63da840b82531fe9f5e1d16a318c` (licence evidence in [`tipnr.md`](tipnr.md)).
- **Archived licence notice:** [`theographic-license.txt`](theographic-license.txt), the repository's `LICENSE` at the pinned commit (the CC BY-SA 4.0 legal code, sha256 `7abe19ec9bb73b36141b999b861d24ad855e808bafe0f81e84cce28556f6c297`), and [`theographic-readme.md`](theographic-readme.md), its `readme.md` (sha256 `725d3262630fa0a7152b643887e00522fc072c10eea92c54621151894423efb4`). Also archived: [`theographic-chronology.md`](theographic-chronology.md) (`docs/chronology-and-dates.md`, how its dates are numbered and where they come from) and [`theographic-bibliography.md`](theographic-bibliography.md) (`docs/data-source-bibliography.md`).

## Licence

The README at the pinned commit says:

> ## License
> This work is free to use and copy under a [Creative Commons Attribution-ShareAlike 4.0 International License](LICENSE). Please also credit the [original sources](docs/data-source-bibliography.md), several of which have their own licenses.

GitHub reports the repository licence as `CC-BY-SA-4.0`. SPDX: `CC-BY-SA-4.0`, a Free to share licence (CONTEXT.md, Licence class). Share-alike: the converted `.obr` is an adaptation and is shared under the same licence (the lock's `spdx`); the attribution names Theographic, its chronology's authors and TIPNR, and the lock's `changes` note records what the conversion changes (CC BY-SA 4.0 §3(a)(1)(B)). TIPNR data (CC BY 4.0) mixed into it only as ids and names is compatible with CC BY-SA.

## The original sources it credits

The bibliography says the event dates and people's birth and death years follow Floyd Nolen Jones, *The Chronology of the Old Testament* (Master Books), and the dates in Acts follow timelines Todd Atwood, ThM, provided to the project. These are dates (facts of a chronology) compiled by Theographic into its CC BY-SA dataset; no text of either work is shipped. The attribution names both, as the README asks.

## What is not read, and why

- **Easton's Bible Dictionary text.** `People.csv`'s `dictText` (and `json/people.json`'s `dictionaryText`) is Easton's 1897 text, which the bibliography says Theographic parsed from the Christian Classics Ethereal Library's XML edition. Under our rule against CCEL-hosted material the converter never reads those columns, and nothing from them reaches the Resource. The pinned files themselves are hosted on GitHub by Theographic, not by CCEL.
- **Notes and groups.** Six events carry free-text `notes`; they are not read. Neither are `groups`.
- **Other tables** (verses with KJV text, places, chapters, books, definitions, Easton) are not downloaded.
