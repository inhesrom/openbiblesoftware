# Evidence: Hebrew Old Testament, STEPBible TAHOT (`tahot`)

- **Edition shipped:** STEPBible-Data "TAHOT - Translators Amalgamated Hebrew OT" (Leningrad Codex as read by translators, with disambiguated Strong's tags for prefixes and suffixes and ETCBC morphology), at commit [`5de0a47f35f0fc13dfe1430d104e5db8a4285f9c`](https://github.com/STEPBible/STEPBible-Data/tree/5de0a47f35f0fc13dfe1430d104e5db8a4285f9c/Translators%20Amalgamated%20OT%2BNT) (2026-11-02, "Licence summary clarified"; the data itself last changed in commit `f35ad49`, 2025-01-29).
- **Pinned files:** raw GitHub URLs at that commit, which never change. STEPBible asks that its data files not be posted elsewhere, so no Internet Archive copy is made.

| File | sha256 |
|---|---|
| `TAHOT Gen-Deu - Translators Amalgamated Hebrew OT - STEPBible.org CC BY.txt` | `7763ef367bb0e6e1c1a95fb9b652e1d611b2481827611433e82abbf229216d70` |
| `TAHOT Jos-Est - Translators Amalgamated Hebrew OT - STEPBible.org CC BY.txt` | `b79add01a8c95673e31ec3ab8ad86183d2312cd96421ddf21f3d1f74377526a7` |
| `TAHOT Job-Sng - Translators Amalgamated Hebrew OT - STEPBible.org CC BY.txt` | `e0416880bb78f46715bb17ce526b623c3051da0ef83f7bf83beaad350e6272e8` |
| `TAHOT Isa-Mal - Translators Amalgamated Hebrew OT - STEPBible.org CC BY.txt` | `d5a609f5a4b9f7757a2615e55877b07f9f6931a8afccef012c0b4bf662caf540` |

- **Archived licence notices:** [`tahot-licence.txt`](tahot-licence.txt), the header of the pinned Gen-Deu file (the other three carry the same notice), trailing whitespace removed; [`stepbible-readme.md`](stepbible-readme.md), the licence section of the repository README at the same commit.

## Licence

The notice in every pinned file says:

> Data created by www.STEPBible.org based on work at Tyndale House Cambridge (CC BY 4.0) … This licence allows you to: Include any part of this data in software or publications … Download the data and reformat it for your application … (You MAY make changes yourself, but you should include a note of changes that can be viewed by those who use your new data) … Attribute the data to Tyndale House and STEPBible.org, and include a link to STEPBible.github.io/STEPBible-Data as the source of this data. Please do not post copies of this data file elsewhere, so that this remains the single up-to-date source

SPDX: `CC-BY-4.0`. How the Starter library meets it:

- **Attribution:** the lock's `attribution` credits Tyndale House and STEPBible.org, links STEPBible.github.io/STEPBible-Data and names the licence; it is shown in About, and `homepage` (the source URL in About) is the same link.
- **Changes:** the lock's `changes` lists what the conversion keeps, turns into notes and drops; About shows it and the build publishes it as `CHANGES-tahot.md` with each release (#20 decision 4).
- **No re-posting:** the build downloads from STEPBible's own repository and ships only the converted `.obr`, never the source files.
