# Atlas (#150)

A Tool that maps the places a Passage names, offline. Decisions: [ADR 0010](../adr/0010-place-locations-ride-on-tipnr-places-and-the-base-map-is-built-in.md). Data sources and licences: [research/atlas-data.md](../research/atlas-data.md).

## Contract

- **Tab.** `TabKind::Tool(Tool::Atlas)`, `tool.atlas {passage?, new_tab?, panel?, split?}` (Tools menu and launcher, the Panel `+`). Header box: a Bible reference. Key schemes: Passage, so it joins Link sets and follows them like the Cross-references Tool. The map shows `atlas::atlas_passage(shown)`: a single Verse widens to its chapter (a Bible at Acts 13:14 maps Acts 13); a typed range or chapter stays as typed. Places the current Verse names are drawn larger.
- **Places.** `atlas::passage_places(library, directory, passage)` (a worker job, cached per Passage in `References::places`): every entity of kind Place that any People & places index lists in the Passage, once each, ordered by the first Verse that names it, then by name. Each is a `MapPlace` with its `PlacePoint`s (most confident first) from the People & places directory, and the Verses of the Passage that name it.
- **Place locations.** `Entity::locations: Vec<PlaceLocation>` (name, `lon_e6`/`lat_e6`, score 0–1000), stored in the `.obr`'s optional `entity_locations` table (older files read none). The directory keeps the first index's record of an entity and takes Place locations from the first index that has some.
- **Confidence.** `Confidence::of(score)`: High ≥ 500 (OpenBible.info's own threshold), Medium 100–499, Low < 100. Markers: filled, half-filled, hollow; the picked place is ringed in gilt and shows all its locations.
- **Search.** `Session::atlas_search(query)`: the directory's name chooser (`PeopleDirectory::choose`), kept to located places. Picking a result puts it on the map (even outside the Passage) and centres there.
- **Map.** `base_map::builtin()` decodes `assets/atlas/basemap.bin` once (format in the module docs). `MapView` (centre and points per degree; equirectangular, longitudes narrowed by cos 36°) does fit, pan, zoom-about-pointer and clamps to the base map's bounds and `MIN_SCALE..MAX_SCALE`. The UI (`shell/atlas_tab.rs`, `map_canvas`) draws land and lakes as meshes, coasts, shores and rivers (by scale rank as you zoom) as lines, then markers and labels placed greedily (right, left, above, below; never over another label or outside the map). Drag pans, the wheel or a pinch zooms, double-click zooms in, `−` `+` `Fit` buttons.
- **Clicks.** A marker or a list row's `Factbook ›` runs `guide.factbook {entity, new_tab}` (Ctrl/Cmd: new Tab). A list row's name picks the place.
- **Factbook Map.** `GuideSection::Map` (`map`, between Summary and Key Passages): a place's every Place location with its Confidence; a person's or group's places they lived in (TIPNR's inhabitant links, `PeopleDirectory::places_lived_in`), each linking to its Factbook. Empty: with no geocoding index, the empty state offers it (`atlas::GEOCODING_INDEXES`); with one, it says no location is known.

## Data

- **Starter library:** lock entry `openbible-geo` (importer `openbible-geocoding`): `ancient.jsonl` and `modern.jsonl` at commit `7eb18a5`, plus the TIPNR file (same pin as `tipnr`) for matching. Converter: `obs_formats::import_openbible_geocoding`. 1,014 places (969 under TIPNR's ids), 1,003 located, 8,601 mentions; 0.6 MB.
- **Base map:** `cargo xtask atlas-basemap` (pinned Natural Earth GeoJSON, sha256-checked, cached in `target/starter-cache`): 9,907 land vertices / 9,185 triangles, lakes 1,039 triangles, 385 coast lines, 58 shores, 242 rivers; 244,750 bytes.

## Evidence

- Formats `tests/openbible_geo.rs`: Acts 13 places take TIPNR's ids with their locations; Acco/Ptolemais merge; ordering and the ODbL/negative-score/no-Verse drops; KJV verse numbers; without TIPNR; determinism. Full sources with `OBS_TEST_OPENBIBLE_GEO` and `OBS_TEST_TIPNR`.
- Engine `atlas_tests`: `acts_13_shows_the_places_of_pauls_first_journey` (the acceptance check), chapter widening, unlocated places, confidence levels, search, the directory merge, the Factbook Map for a place and a person, the Map's empty states, the Tool's Tab. `base_map::tests`: the built-in file decodes, covers Rome to Susa, Jerusalem is on land and the Mediterranean is not, views fit/pan/zoom.
- UI `shell::tests::atlas`: Acts 13 labels sit where the places are and a marker click opens Iconium's Factbook with its Map; the search box; following a Link set to Acts 21.
- Real Starter library (manual): `OBS_LIBRARY=dist/starter cargo test -p obs-engine --lib starter_library_acts_13_atlas -- --ignored --nocapture`.
