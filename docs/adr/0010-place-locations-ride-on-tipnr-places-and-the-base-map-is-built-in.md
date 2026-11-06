# Place locations ride on TIPNR's places, and the base map is built into the app

The Atlas (#150) needs to know where the places a Passage names were, how sure that is, and a map to draw them on, offline. OpenBible.info's Bible Geocoding Data (CC BY 4.0) has the locations and their confidence; Natural Earth (public domain) has the land, lakes and rivers. We decided two things that are hard to undo once Starter libraries and Layouts carry them:

1. **The geocoding is a People & places index, and its places take TIPNR's ids.** It ships in the Starter library as `openbible-geo`. Each place's modern locations become **Place locations** on the entity (`Entity::locations`, an optional `entity_locations` table in the `.obr`, like `entity_strongs`). The Starter build matches each OpenBible place to the TIPNR place with its name that shares the most Verses with it and gives it TIPNR's id; places TIPNR counts as one (Acco and Ptolemais) merge. The People & places directory keeps TIPNR's record of a place and takes the geocoding's Place locations for it, so a place is one entity with one Factbook, whichever index locates it. Places TIPNR lacks keep an id of their own (`Name@OpenBible.a1b2c3d`) when they have a location.
2. **The base map is vector data compiled into the app** (`crates/engine/assets/atlas/basemap.bin`, 245 KB): Natural Earth's 1:10m land and natural lakes cut to the Bible lands, simplified and triangulated by `cargo xtask atlas-basemap`, plus coasts, shores and rivers as lines. egui's painter draws it as meshes and lines; there is no tile server and no network.

## Considered Options

- **A seventh Resource type ("Atlas data", "Gazetteer"):** says plainly what it is, but every place that lists Resource types would gain a case and an older app could not open the `.obr` (the reasons ADR 0009 gave). The geocoding is exactly what CONTEXT.md calls a People & places index: it identifies places and lists the Passages that name them.
- **Geocoding places under their own ids, matched to TIPNR at run time:** keeps the importer independent of TIPNR, but the Passage Guide's People & places and the Factbook directory would list each place twice (TIPNR's Antioch and OpenBible's), and every lookup would repeat a name-and-Verse match.
- **Coordinates from TIPNR itself:** TIPNR's place records carry map links with coordinates, but no confidence, one point per place, and the converter deliberately drops them with the other unstructured columns (#43). The ticket asks for confidence levels.
- **Map tiles (raster or vector) from a tile server, or OpenStreetMap data:** online, or a large download; OpenStreetMap data is ODbL (share-alike for databases), which `open-catalogue.md` keeps separate from what we ship.
- **The base map as a Starter library Resource:** would count against the 400 MB budget (now 398.5 MB) and be missing from an app without its Starter library; as built-in data it is 245 KB of the binary and always there.

## Consequences

- The geocoding's importer reads TIPNR's file too (a third pinned file in its lock entry). A new TIPNR revision rebuilds the geocoding with it; a TIPNR id that disappears simply leaves that place under OpenBible's id.
- 969 of the geocoding's 1,014 kept places carry TIPNR's id; 45 keep their own. OpenStreetMap-derived coordinates (51 of 1,596 modern locations) are not shipped, so 7 places (Azekah, Zarephath, …) are known but unlocated.
- `STARTER_PREFERRED_ORDER` puts TIPNR before `openbible-geo`, so TIPNR's record of a place is the one the Factbook and the Passage Guide show. A user who ranks the geocoding first still gets one entity per place, with the geocoding's (thinner) record.
- An imported People & places index with Place locations works in the Atlas as it is; the Map section's empty state offers only the indexes in `atlas::GEOCODING_INDEXES`.
- Changing the base map's region or detail is a rerun of `cargo xtask atlas-basemap` and a binary diff; the file's layout is versioned by its magic (`OBSMAP\x01`).
