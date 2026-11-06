# Evidence: OpenBible.info Bible Geocoding Data (`openbible-geo`)

- **Edition shipped:** the `openbibleinfo/Bible-Geocoding-Data` repository at commit `7eb18a5ee62f27b9b93bd6689ea272d76dd23b8f` (2021-10-31, the repository's latest commit when checked on 2026-11-06): `data/ancient.jsonl` (1,342 places as the Bible names them) and `data/modern.jsonl` (1,596 modern locations).
- **Pinned files** (GitHub raw URLs at a commit, so their bytes cannot change):
  - <https://raw.githubusercontent.com/openbibleinfo/Bible-Geocoding-Data/7eb18a5ee62f27b9b93bd6689ea272d76dd23b8f/data/ancient.jsonl>, **sha256** `b8187aa4737e8517ccc090f765d2be11da4c548cd2a59d3cdcb62e952cb8c0f2` (11,550,193 bytes)
  - <https://raw.githubusercontent.com/openbibleinfo/Bible-Geocoding-Data/7eb18a5ee62f27b9b93bd6689ea272d76dd23b8f/data/modern.jsonl>, **sha256** `da731f6e110bac4ea66a9f037a0a31cfb11c4f1efc1206aa9e109092b2c60087` (3,224,520 bytes)
  - TIPNR, read only to match places to TIPNR's ids: the same file and pin as the `tipnr` entry, **sha256** `d595b9163d32a400bb0ef96b27a6de58335a63da840b82531fe9f5e1d16a318c` (licence evidence in [`tipnr.md`](tipnr.md)).
- **Archived licence notice:** [`openbible-geo-license.txt`](openbible-geo-license.txt), the repository's `license.txt` at the pinned commit (the CC BY 4.0 legal code, sha256 `f5b745ef98087f531e719ee8ca6a96809444573ecc7173c6fa68eaad39b3cc3f`), and [`openbible-geo-readme.md`](openbible-geo-readme.md), its `readme.md` (sha256 `dfd6e967ffdd139ea2cf3222ed2ca0f56d44da6653247a624aece279ad85e782`).

## Licence

The README at the pinned commit says:

> ## License
>
> This data is licensed under a [Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/) license.
>
> OpenStreetMap data is licensed under [ODbL 1.0](https://opendatacommons.org/licenses/odbl/), which is similar to CC-BY-SA.
>
> The licenses for images vary depending on the image but are open and are generally Creative Commons or similar.

GitHub reports the repository licence as `CC-BY-4.0`. SPDX: `CC-BY-4.0`. Attribution credits OpenBible.info with a link to <https://www.openbible.info/geo/>; the lock's `changes` note and the published change record state what the conversion changes (CC BY 4.0 §3(a)(1)(B)).

## What is not shipped, and why

- **OpenStreetMap coordinates (ODbL).** `modern.jsonl` names where each location's coordinates come from (`coordinates_source.type`). 51 of the 1,596 locations take theirs from OpenStreetMap (`osm`). ODbL is share-alike for databases, so, as [open-catalogue.md](../../docs/research/open-catalogue.md) decided ("Keep ODbL data separate"), the converter drops those coordinates. Seven places lose their only location this way (Alush, Azekah, Etam, Gedor, Gozan, Karka, Zarephath); they stay in the index, unlocated, where TIPNR knows them.
- **Geometry, images and thumbnails.** Region and river outlines (`geometry.jsonl`, GeoJSON files, partly OpenStreetMap), photos (each with its own licence) and the 180 MB thumbnails archive are not read.
- **Prose.** Comments, descriptions and accuracy notes are not read; only names, Verses, coordinates and scores.
- **Links to other datasets** (`linked_data`) are not shipped; the TIPNR link only helps match a place to TIPNR's id.
