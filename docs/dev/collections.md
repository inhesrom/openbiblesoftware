# Collections (#153)

A **Collection** is a named set of Resources the user saves in the Library Tab. It can be
chosen as a Library search's Search scope, as the Passage Guide's Commentaries filter and as
the Library Tab's filter. A **Resource tag** is the user's own label on a Resource, for
`tag:` rules.

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `obs-engine` | `collections` | `Collection` (name, rules, Resources chosen by hand), `CollectionRule`, `parse_rules`/`rules_text`, membership (`Library::in_collection`, `Library::collection_members`), loading and saving, and the commands `collection.save`, `collection.toggle`, `collection.delete`, `collection.list` and `resource.tags`. |
| `obs-engine` | `Library` | Holds the `CollectionSet` (Collections and Resource tags) beside the Preferred order, so every holder of a `&Library` (Search scope, Guide jobs, the Library Tab) can test membership. |
| `obs-engine` | `library_search::LibraryScope` | `collections` beside `types` and `resources`; `search.collection` toggles one; `search.run`'s `resources` takes `collection:<name>`. |
| `obs-engine` | `guide::GuideArrangement` | `filters`: section → Collection, stored with the Guide arrangement; `guide.filter_section`; `Session::guide_section` keeps only the filtered Collection's Commentary rows. |
| `obs-engine` | `LibraryFilter` | `collection`; `matches_in(&library, manifest)`. |
| `obs-ui-egui` | `shell/collections_ui.rs` | The Library's Collection chips, New Collection… / Edit… window and a row's Collections submenu. |
| `obs-ui-egui` | `search_tab.rs`, `guide_tab.rs`, `library_tab.rs` | Choose… lists the Collections and the chips show `Collection: <name>`; the Commentaries section's Collection chooser; the About Tab's Tags field. |

## Rules

Typed as `field:value` pairs separated by spaces; a value with spaces goes in double quotes.

| Field | Matches |
|---|---|
| `type:` | The Resource type: `bible`, `commentary`, `lexicon`, `dictionary`, `cross-reference-set`, `people-and-places` (singular or plural). |
| `language:` (`lang:`) | The language's English name (`greek`) or its BCP 47 tag (`grc`, `en`). |
| `tag:` | A Resource tag the user gave the Resource (any case). |
| `licence:` (`license:`) | Part of the SPDX licence, any case (`cc-by` matches `CC-BY-4.0` and `CC-BY-SA-4.0`). |
| `title:` | Part of the title or abbreviation, any case. |

Rules of different fields must all match; several rules of one field match when any does:
`type:lexicon type:dictionary language:greek` is every Greek Lexicon or Dictionary. A
Collection's members are the Resources chosen by hand plus every library Resource its rules
match (a Collection with no rules is by hand only). Rules are evaluated against the library
each time, never stored as a member list, so a Commentary downloaded from Get Resources or
imported joins `type:commentary` at once.

## Where a Collection is used

- **Search scope (Library mode).** Choose… lists the Collections above the types. Choosing a
  Collection while every Commentary, Lexicon and Dictionary is searched (the default)
  searches just that Collection; otherwise it adds to the scope. Its Bibles, Cross-reference
  sets and People & places indexes are left out (Library mode searches Commentaries, Lexicons
  and Dictionaries). Leaving one of its Resources out (×, or unticking it) drops everything
  that covered it (its whole type, every Collection it is in, its own choice) and keeps the
  others those covered one by one. Saving, editing, deleting a Collection, adding or taking
  out a Resource, or changing a Resource tag a `tag:` rule uses, runs again every Library
  search whose scope names that Collection (cancelling a search still running for the old
  scope). Saved searches (Favorites, History) keep the Collections.
- **Passage Guide filter.** Once the user has a Collection, the Commentaries section shows a
  Collection chooser (`Every Resource` or one Collection). The choice is part of the Guide
  arrangement (every Passage Guide, kept across restarts; Reset arrangement clears it). A
  filter that leaves nothing says so instead of the "None in your library" offers.
- **Library filter.** The Collection chips under the language chips narrow the list (one
  Collection at a time; All clears it), together with the header text and language chips.

Renaming a Collection (Edit…) follows it in open Search Tabs and Guide filters; deleting it
drops it from them. A Resource removed from the library stays in a by-hand Collection's list
and is back in it if imported again. Edit… lists it under "Not in the Library now" to keep or
untick; `collection.save` accepts such ids only for the Collection that already holds them
(a newly chosen id must be in the library), and `collection.toggle` can take one out.

## Storage

The User database's `preference` table: `library.collections` (JSON list of Collections,
rules as tagged values such as `{"resource_type":"commentary"}`) and `library.tags` (JSON
map of resource id → tags), the way Copy presets are kept. No schema migration.

Previews (harness screenshots): [New Collection…](../previews/collections-new-kittest.png),
[the Library filtered](../previews/collections-library-filter-kittest.png),
[a Library search scoped to it](../previews/collections-search-scope-kittest.png) and
[the Passage Guide's Commentaries filtered](../previews/collections-guide-filter-kittest.png).
To make them again: `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui --lib collections`, which
writes `target/previews/collections-*.ppm`.
