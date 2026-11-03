# Evidence: Greek New Testament, STEPBible TAGNT (`tagnt`)

- **Edition shipped:** STEPBible-Data "TAGNT - Translators Amalgamated Greek NT" (every word of the major Greek editions, with translations, grammar, editions and meaning variants), at commit [`5de0a47f35f0fc13dfe1430d104e5db8a4285f9c`](https://github.com/STEPBible/STEPBible-Data/tree/5de0a47f35f0fc13dfe1430d104e5db8a4285f9c/Translators%20Amalgamated%20OT%2BNT) (2026-11-02, "Licence summary clarified"; the data itself last changed in commit `051e2bc`, 2024-12-08). The Starter Bible's text is the words TAGNT marks as in NA27.
- **Pinned files:** raw GitHub URLs at that commit, which never change. STEPBible asks that its data files not be posted elsewhere, so no Internet Archive copy is made.

| File | sha256 |
|---|---|
| `TAGNT Mat-Jhn - Translators Amalgamated Greek NT - STEPBible.org CC-BY.txt` | `d4c0c820fdf9f2b450c9f96ce6103d6875028affd2e44094d753f1329ca1fd85` |
| `TAGNT Act-Rev - Translators Amalgamated Greek NT - STEPBible.org CC-BY.txt` | `122040eeeaeba50d6abbf67b39cacaff687211bda69b080e5d53927c830d9968` |

- **Archived licence notices:** [`tagnt-licence.txt`](tagnt-licence.txt), the header of the pinned Mat-Jhn file (the Act-Rev file carries the same notice), trailing whitespace and byte-order mark removed; [`stepbible-readme.md`](stepbible-readme.md), the licence section of the repository README at the same commit.

## Licence

The notice in both pinned files says:

> Data created by www.STEPBible.org based on work at Tyndale House Cambridge (CC BY 4.0) … This licence allows you to: Include any part of this data in software or publications … Download the data and reformat it for your application … (You MAY make changes yourself, but you should include a note of changes that can be viewed by those who use your new data) … Attribute the data to Tyndale House and STEPBible.org, and include a link to STEPBible.github.io/STEPBible-Data as the source of this data. Please do not post copies of this data file elsewhere, so that this remains the single up-to-date source

SPDX: `CC-BY-4.0`. The [open content inventory](../../docs/research/open-content-inventory.md) lists TAGNT as CC BY 4.0 data (#2). How the Starter library meets the licence:

- **Attribution:** the lock's `attribution` credits Tyndale House and STEPBible.org, links STEPBible.github.io/STEPBible-Data and names the licence; it is shown in About, and `homepage` (the source URL in About) is the same link.
- **Changes:** the lock's `changes` lists what the conversion keeps, turns into notes and drops; About shows it and the build publishes it as `CHANGES-tagnt.md` with each release (#20 decision 4).
- **No re-posting:** the build downloads from STEPBible's own repository and ships only the converted `.obr`, never the source files.
