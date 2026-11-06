# Favorites and History (#155)

Two ways back to places the user cares about, across all Tabs. Terms are in
[CONTEXT.md](../../CONTEXT.md): **Favorite**, **Favorites folder**, **History**, **Visit**.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `favorites.rs`, `favorites_store.rs` | User database schema 7: the `favorite` and `history` tables. History forgets Visits older than `HISTORY_DAYS` (90) and keeps at most `HISTORY_LIMIT` (5,000), pruned on every write. Each Visit stores its local day (SQLite `localtime`) so History can group by day without a time-zone crate. |
| `obs-engine` | `favorites.rs` | `TabLocation` (Tab kind + Current location), `Session::location_of`, `open_location`, `record_history`, `favorites_matching`, `history_days`; commands `favorite.add`, `favorite.open`, `favorite.move`, `favorite.remove`, `history.open`, `history.clear`, `tool.favorites`, `tool.history`. |
| `obs-engine` | `workspace.rs` | `PendingVisit`: every Tab opened or jumped is queued for History, with where the jump went (`VisitedAt`: Passage, entry Key, Search fields). |
| `obs-ui-egui` | `shell/favorites_tab.rs` | The ☆ in each Tab header, the ⋯ menu's "Add to Favorites folder", the Favorites and History Tools; Library → Favorites lists them as a menu. |

## Where a Tab is: `TabLocation`

A Favorite and a Visit both store a `TabLocation` as JSON:

- **Resource**: its id plus a Passage (`verse`, in that Resource's own numbering) or, for a
  Lexicon or Dictionary, the entry Key (`key`).
- **Passage Guide, Exegetical Guide, Text Comparison, Cross-references**: a Passage in the Standard versification.
- **Word Study**: its Strong's number (`key`).
- **Search**: the Search Tool's fields (`SavedSearch`: mode, query, Scope, chosen Bibles,
  Library mode's Resource types and Resources, Notes mode's Notebook).

Keyless Tabs (Library, Notes, Word Info, About, Get Bibles, Credits, the Favorites and History
Tools) have no location: they have no ☆, and `favorite.add` refuses them.

The label ("Romans 8:28 · BSB", "G26 · Word Study", "“love” · Bible Search") is made when the
Favorite or Visit is saved, so it still reads well if the Resource later leaves the library.

## Opening one

`favorite.open` / `history.open` (`id`, optional `panel`, `split`) run
`Session::open_location`:

1. A Resource no longer in the library fails with "… is no longer in your library".
2. A Search runs again in the most recent Search Tab (as `search.run` would), with every
   saved field restored: no chosen Bibles means the ★ Bible, not that Tab's last choice.
3. A Word Study moves the most recently active Word Study Tab (`word_study::open_at`).
4. Otherwise the most recently active Tab of that kind moves there (a jump: it gets a Back
   entry and its Link set follows). With none, or with `split`, a new Tab opens: a new
   Resource Tab starts at the location once the UI loads it, then jumps there
   (`Effect::NavigateOpened`, applied once its Verse map is known) so the Link set it
   joined from the focused Tab follows, each member in its own numbering; a Guide or Tool
   jumps there.

## Recording History

`Workspace::navigate`, `navigate_text`, `navigate_entry`, `navigate_standard`, Back, Forward
and `Workspace::open` queue a `PendingVisit`; `Session::run_search` queues one for the Search
Tab. A jump captures where it went at once (a Search its fields, so several searches recorded
together each stay a Visit); a new Tab is read later, once the UI has placed it. A new
Resource Tab with no location yet (a first Bible gets one only once its text is laid out) stays
queued until it has one, unless it jumps first. A Tab closed (or replaced by `layout.restore`)
before its Visits are recorded is kept aside (`Workspace::visited_tab`) until they are, so
they are recorded where it was. When a Visit cannot be written, it and every Visit after it
stay queued in order, the failure is shown once, and they are tried again after
`HISTORY_RETRY_SECS` (5 s). The shell calls `Session::record_history` at
the end of every `apply_effects`, and `save_last_session` calls it too (for headless use). A
Visit equal to the previous one is skipped. `history.clear` drops the queued Visits as well.
Day headings ("Today", "Yesterday") are worked out again when the local day changes, checked
with each Last session save (at most once a second). Scrolling and Link-set following make no Visits, so History lists places the user
went to, not every Verse scrolled past.

## Favorites folders

`favorite.add` takes an optional `folder`; a new name makes a folder. Folder names ignore case
(the first spelling is kept). A folder exists while it holds Favorites; `favorite.move` with
an empty folder takes a Favorite out of its folder.

## Hand-offs

- The Command box (#139) suggests `Session::favorites_matching(text)` and opens one with
  `favorite.open`.
- The toolbar Library pane (#140) lists `Session::favorites()` (grouped by `folder`) the same
  way Library → Favorites does today.
