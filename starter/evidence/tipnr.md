# Evidence: TIPNR People & places (`tipnr`)

- **Edition shipped:** STEPBible-Data `Proper Nouns/TIPNR - Translators Individualised Proper Names with all References - STEPBible.org CC BY.txt`, last changed in commit `be4191f` (2026-10-24), pinned at commit `5de0a47f35f0fc13dfe1430d104e5db8a4285f9c` (2026-11-02).
- **Pinned file:** <https://raw.githubusercontent.com/STEPBible/STEPBible-Data/5de0a47f35f0fc13dfe1430d104e5db8a4285f9c/Proper%20Nouns/TIPNR%20-%20Translators%20Individualised%20Proper%20Names%20with%20all%20References%20-%20STEPBible.org%20CC%20BY.txt>. The URL names a git commit, so its bytes cannot change.
- **sha256:** `d595b9163d32a400bb0ef96b27a6de58335a63da840b82531fe9f5e1d16a318c`
- **Archived licence notice:** [`tipnr-stepbible-readme.md`](tipnr-stepbible-readme.md), the repository `README.md` at the pinned commit, plus the licence block inside the pinned file (quoted below).

## Licence

The licence block at the top of the pinned file says:

> Data created by www.STEPBible.org based on work at Tyndale House Cambridge (CC BY 4.0)
>
> This licence allows you to:
> * Include any part of this data in software or publications and send it to AI servers without requesting permission
> * Download the data and reformat it for your application
> * Send any proposed corrections to STEPBibleATGmail.com to be verified
>   (You MAY make changes yourself, but you should include a note of changes that can be viewed by those who use your new data)
> * Refer others to STEPBible.github.io/STEPBible-Data as the source of the data.
>   Please do not post copies of this data file elsewhere, so that this remains the single up-to-date source"

The README at the pinned commit says the repository is **CC BY 4.0** and asks users to "Credit it to "STEP Bible" linked to www.STEPBible.org" and to record changes and make them available.

SPDX: `CC-BY-4.0`. The Starter build reformats the data into a People & places `.obr` (it does not republish the TIPNR file); Attribution credits "STEP Bible" with a link, and the lock's `changes` note records what is kept and omitted.

## AI-written descriptions are not shipped

TIPNR's field notes say the `@Brief`, `@Short` and `@Article` descriptions are "Adapted from the output by Claude 3 Opus AI in April 2024", and the README says every individual "has a description in brief, short and article length (created by Claude 3 AI)". Following the Passage Guide decision (#15), the converter keeps only structured fields and never copies any description column (`Description`, `#Summary description`, `@Briefest`, `@Brief`, `@Short`, `@Article`); it fails the build if any description prose reaches a kept field.
