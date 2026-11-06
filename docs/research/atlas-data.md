# Atlas data: places, locations and a base map (#150)

Checked live on 2026-11-06 from this host. Decisions: [ADR 0010](../adr/0010-place-locations-ride-on-tipnr-places-and-the-base-map-is-built-in.md).

## Where each place was: OpenBible.info Bible Geocoding Data

- **Source:** <https://github.com/openbibleinfo/Bible-Geocoding-Data>, latest commit `7eb18a5ee62f27b9b93bd6689ea272d76dd23b8f` (2021-10-31; the repository has three commits, all that day). `data/ancient.jsonl` (11.6 MB, 1,342 places), `data/modern.jsonl` (3.2 MB, 1,596 modern locations), plus `geometry.jsonl`, `image.jsonl`, `source.jsonl` and GeoJSON/KML files we do not use.
- **Licence:** README: "This data is licensed under a Creative Commons Attribution 4.0 license. OpenStreetMap data is licensed under ODbL 1.0 … The licenses for images vary". `license.txt` is the CC BY 4.0 legal code; GitHub reports `CC-BY-4.0`. Archived in `starter/evidence/openbible-geo-*`.
- **What it gives:** for each place, every Verse that names it (ESV numbering, with `alternate_verses.kjv` where the KJV puts the name in another Verse: 2 cases), its identifications with modern locations and a score per location (`modern_associations`; "500 or higher represents high confidence"), the spellings ten English translations use, and links to other datasets (TIPNR among them).
- **ODbL:** each modern location says where its coordinates come from. Of 1,596: Wikidata 938, DAAHL 198, MEGA-Jordan 113, GeoNames 84, IAA 63, **OpenStreetMap 51**, Palestine Open Maps 43, others under 20 each. We drop the 51 OpenStreetMap ones; 7 places then have no location (Alush, Azekah, Etam, Gedor, Gozan, Karka, Zarephath).
- **Scores:** of 4,314 place–location associations, 408 score below 0 (judged wrong) and 368 exactly 0 (unknown); both are dropped. 3 places lose every location that way.
- **Not used:** images (each with its own licence; thumbnails are a 180 MB archive), geometry (regions and rivers, partly OpenStreetMap), prose comments.

### Matching to TIPNR

The data links 1,221 of its 1,285 Verse-bearing places to a TIPNR id, but to the 2021 ids (`Abdon@Jos.21.30`); only 387 of those are still ids in the pinned TIPNR (`Abdon@Jos.19.28-1Ch` today). So the converter matches by name and Verses instead: the TIPNR place called by the OpenBible place's TIPNR-link name, its own name without OpenBible's number (`Antioch 2`), or a spelling Bibles use, that shares the most Verses with it. Result on the real files: 1,182 of the 1,285 places find their TIPNR place; TIPNR counts some pairs as one place (Acco and Ptolemais, Atad and Abel-mizraim), so they make 969 entities; 45 more places keep OpenBible's id because they have a location; the rest (TIPNR does not know them and no location survives) are dropped. Every place of Acts 13 matches.

TIPNR's own place records carry Google Maps and Palestine Open Maps links with coordinates, one point each and no confidence; the converter drops them (#43), and they could not show confidence anyway.

### Acts 13 in the built Starter library

`OBS_LIBRARY=dist/starter cargo test -p obs-engine --lib starter_library_acts_13_atlas -- --ignored --nocapture`: 15 places in 0.16 s, all High confidence: Antioch (Antioch on the Orontes, 13:1), Cyrene (13:1), Cyprus, Seleucia (Seleucia Pieria, 13:4), Salamis (13:5), Paphos (New Paphos, 13:6, 13), Jerusalem (13:13, 27, 31), Pamphylia, Perga (13:13-14), Antioch (Antioch in Pisidia, 13:14), Pisidia (drawn at Pisidian Antioch), Egypt (13:17), Canaan (13:19), Galilee (13:31), Iconium (Konya, 13:51).

## The base map: Natural Earth

- **Source:** Natural Earth 1:10m physical vectors as GeoJSON from <https://github.com/nvkelso/natural-earth-vector> at commit `ca96624a56bd078437bca8184e78163e5039ad19`: `ne_10m_land` (10.2 MB, sha256 `1ac90796408bc6ad6911d69448485d3c4dbf2190370080368a09976e1c9f7416`), `ne_10m_lakes` (5.0 MB, `2d036f53dedec578001c5c30c2959ee7d4eebc1306900fa4367c49929ec8f2d9`) and `ne_10m_rivers_lake_centerlines` (7.3 MB, `bb854a900ecbd3b408df46d5e16e3e0f974ba55993f9d8b5c26e855273c0905a`), pinned in `xtask/src/atlas_basemap.rs`.
- **Licence:** the repository's `LICENSE.md`: "Everything here is public domain … No permission is needed to use Natural Earth. Crediting the authors is unnecessary." We credit it anyway (Help → Credits, from `ATTRIBUTION.toml`).
- **What we keep:** 12°W–72°E, 6°N–50°N (Tarshish in Spain to India, Ethiopia to the Black Sea: every place's most confident location lies inside; the data's extremes are Spain at 6.9°W, India at 67.4°E, Punt at 11.6°N and Scythia at 44.9°N). Land and natural lakes (Natural Earth marks reservoirs such as Lake Nasser, which are modern, and we leave them out), rivers of scale rank 7 or less (the Nile, Euphrates, Tigris and Jordan; the Jordan is rank 6). No modern borders.
- **Size:** simplified at 0.012° and rounded to 1/200° (about 550 m), land and lakes triangulated at build time: 244,750 bytes in the binary. Natural Earth's 1:50m land would be smaller but loses the Sea of Galilee and the Dead Sea's shape at the zooms the Atlas uses for Israel.
- **Rejected:** map tiles (online, or hundreds of MB for offline zooms); OpenStreetMap extracts (ODbL); shipping the base map as a Starter Resource (counts against the 400 MB budget, now at 398.5 MB, and missing without the Starter library).

## Open catalogue instead?

The ticket allowed either. The geocoding is in the Starter library because the Atlas and the Factbook's Map should work from first launch with nothing to download (0.6 MB plus caches). The Open catalogue's importers read SWORD and USFM-like texts; the geocoding needs TIPNR beside it at build time to share its ids, which the Starter build has.
