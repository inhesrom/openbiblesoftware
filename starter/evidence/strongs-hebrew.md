# Evidence: Strong's Hebrew Dictionary (`strongs-hebrew`)

- **Edition shipped:** openscriptures/strongs `hebrew/StrongHebrewG.xml` at commit `0acd2f251c2d35ff8db2dece4e0593979d3ac223` (2021-07-15): James Strong, *A Concise Dictionary of the Words in the Hebrew Bible* (1890/1894), OSIS edition by David Troidl and David Instone-Brewer (revision 2010-05-24).
- **Pinned file:** <https://raw.githubusercontent.com/openscriptures/strongs/0acd2f251c2d35ff8db2dece4e0593979d3ac223/hebrew/StrongHebrewG.xml>. A raw file at a fixed commit never changes.
- **sha256:** `1f9659ea208f4c498843a0280dacb1448627c33ca77712642d8705793ab66061`
- **Archived licence notice:** [`strongs-hebrew-header.xml`](strongs-hebrew-header.xml), the OSIS header of the pinned file (lines 1–61).

## Licence

The OSIS header declares, for Strong's Hebrew dictionary (`osisWork="Strong"`, original date 1894):

> `<rights>Public Domain</rights>`

and describes the edition: "The data came from several PD sources which were full of errors. I cleaned them by comparing them to each other and (where necessary) looking at the original." Strong's dictionary was published in 1890, so it is public domain by age as well.

SPDX: `LicenseRef-PublicDomain`.

## What we do not ship, and why

Each entry also carries a numbered sense outline (`<list>`) and a list of Greek-equivalent Strong's numbers (`<foreign>`). Neither is part of Strong's printed text. The outline is the same text as STEPBible's TBESH "Meaning" column, which STEPBible attributes to the Abridged BDB "© Larry Pierce of OnlineBible.net" and says needs Online Bible's permission (see [`tbesh-header.txt`](tbesh-header.txt)); the Greek equivalents have no stated source. Both are left out, and counted in the build report. The editors' typo notes are left out too. What ships is Strong's own text: headword, transliteration, pronunciation, derivation, definition and KJV renderings.
