# Evidence: Translators Brief lexicon of Extended Strongs for Hebrew (`tbesh`)

- **Edition shipped:** STEPBible-Data `Lexicons/TBESH - Translators Brief lexicon of Extended Strongs for Hebrew - STEPBible.org CC BY.txt` at commit `5de0a47f35f0fc13dfe1430d104e5db8a4285f9c` (2026-11-02).
- **Pinned file:** <https://raw.githubusercontent.com/STEPBible/STEPBible-Data/5de0a47f35f0fc13dfe1430d104e5db8a4285f9c/Lexicons/TBESH%20-%20Translators%20Brief%20lexicon%20of%20Extended%20Strongs%20for%20Hebrew%20-%20STEPBible.org%20CC%20BY.txt>. A raw file at a fixed commit never changes.
- **sha256:** `848eef2ac16753a43694897dbb9e8f266725e7cc9da4809f367ee5b990f05dbb`
- **Archived licence notices:** [`tbesh-header.txt`](tbesh-header.txt) (the file's own header, licence block and field notes, lines 1–52 of the pinned file) and [`stepbible-data-readme.md`](stepbible-data-readme.md) (the repository README at the same commit).

## Licence

The header of the pinned file says:

> Data created by www.STEPBible.org based on work at Tyndale House Cambridge (CC BY 4.0) … You MAY make changes yourself, but you should include a note of changes that can be viewed by those who use your new data … Attribute the data to Tyndale House and STEPBible.org, and include a link to STEPBible.github.io/STEPBible-Data as the source of this data.

SPDX: `CC-BY-4.0`. The changes we make are recorded in the lock's `changes` (shown in About).

## What we do not ship, and why

The same header says of the **Meaning** column:

> These are based on the Abridged BDB by Online Bible, © Larry Pierce of OnlineBible.net … They are for guidance only. Permission should be gained from Online Bible before these are applied in any project.

We have no such permission, so the lock skips the Meaning column (`skip_columns = ["Meaning"]`). What ships is STEPBible's and Tyndale House's own work: the extended and disambiguated Strong's numbers, the Hebrew form ("based on BDB, then normalised"; BDB itself is public domain), the transliteration, and the gloss ("created by Tyndale scholars"). The full definitions for Hebrew come from BDB (`bdb`) and Strong's (`strongs-hebrew`) instead.
