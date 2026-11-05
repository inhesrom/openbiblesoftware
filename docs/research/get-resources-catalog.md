# Get Resources: live SWORD catalog check (issue #132)

**Question:** Which Lexicons, Commentaries and Dictionaries do the SWORD repositories on CrossWire's master list offer, are the Featured candidates named in #132 there under those names and licences, and does SWORD Import read them?
**Date:** run from a Linux host against the live repositories while building #132.
**How:** `curl` of `https://crosswire.org/ftpmirror/pub/sword/masterRepoList.conf`, then each repository's `mods.d.tar.gz` (HTTPS where the master list gives one, FTP for IBT and STEP Bible), read with a small script. Then the ignored live test `live_every_featured_lexicon_commentary_and_dictionary_downloads_and_imports` (`apps/openbible/src/live_tests.rs`) downloaded every Featured module through the app's own transport and imported it.

## Repositories on the master list

All 11 repositories answered: CrossWire (428 confs), CrossWire Beta (11), Bible.org (5), Xiphos (84), CrossWire Attic (88), IBT (155), CrossWire Wycliffe (44), eBible.org (1530), Deutsche Bibelgesellschaft (1), Lockman Foundation (4) and STEP Bible (61).

Unlocked modules by what SWORD Import makes of them (before removing modules that appear in more than one repository):

| Kind | Count | Notes |
|---|---|---|
| Bible | 2065 | |
| Commentary, imports | 54 | KJV or KJVA numbering |
| Commentary, refused | 13 | numbering Catholic (FreAug, FreChry, FreJer, VieLCCMNCT), DarbyFr (FreCJE), NRSV (Sentiment, VarApp ×2, NETnote), Vulg (VulgGlossa), Synodal (BARC, Lopukhin, NGSB) |
| Commentary, `HREFCom` | 0 | none on any repository today; still handled |
| Lexicon (`Feature=GreekDef`/`HebrewDef`) | 28 | |
| Dictionary | 118 | |
| Daily devotional (refused) | 21 | `Category=Daily Devotional` |
| General book (not listed) | 106 | |
| Locked | 6 | 4 Bibles, 1 Commentary, 1 Lexicon/Dictionary |

After removing duplicates, the app's catalog lists 27 Lexicons, 59 Commentaries (12 shown disabled) and 136 Dictionaries (20 shown disabled).

## Featured candidates from #132

| Module | Found | Type | Licence in conf | Imports | Featured |
|---|---|---|---|---|---|
| StrongsGreek | CrossWire (zLD, v2.0) | Lexicon (GreekDef) | Public Domain | yes, 5741 entries; Word Study on G26 shows its entry | yes |
| StrongsHebrew | CrossWire (RawLD, v1.2), CrossWire Beta (v3.0) | Lexicon (HebrewDef) | Public Domain | yes, 8674 entries (CrossWire copy) | yes |
| Thayer | **not on any repository** (Thayer is only mentioned inside the StrongsGreek and GerStrongsGreek confs) | | | | no |
| AbbottSmith | CrossWire (zLD, v4.1) | Lexicon (GreekDef) | Public Domain | **no**: keyed by Greek lemma, not Strong's number, so Import finds no entries ("SWORD module `AbbottSmith` has no entries") | no (follow-up) |
| BDBGlosses_Strongs | CrossWire (zLD, v1.1) | Lexicon (HebrewDef) | Public Domain | yes, 8674 entries | yes |
| MHC | CrossWire (zCom4, KJV) | Commentary | Public Domain | yes, 5504 sections | yes |
| JFB | CrossWire (zCom4, KJV) | Commentary | Public Domain | yes, 18137 sections | yes |
| Barnes | CrossWire, CrossWire Attic (zCom, no Versification = KJV) | Commentary | Public Domain | yes, 7457 sections | yes |
| Clarke | CrossWire (zCom, KJV) | Commentary | Public Domain | yes, 22306 sections | yes |
| Gill | **Xiphos only** (zCom) | Commentary | **none**; its About says "All Rightes Reserved, Larry Pierce" | not tried | no |
| TSK | CrossWire (zCom), CrossWire Beta; IBT has an unrelated Bible also named `TSK` | Commentary | Public Domain | yes, 31152 sections | yes |
| Easton | CrossWire (zLD) | Dictionary | Public Domain | yes, 3961 entries | yes |
| Smith | CrossWire (RawLD, v1.3), CrossWire Beta (v2.0) | Dictionary | Public Domain | yes, 4561 entries | yes |
| ISBE | CrossWire (zLD) | Dictionary | Public Domain | yes, 9380 entries | yes |
| Nave | CrossWire (zLD) | Dictionary | Public Domain | yes, 5320 entries | yes |
| Hitchcock | CrossWire (zLD) | Dictionary | Public Domain | yes, 2612 entries | yes |

Every Featured module comes from the main CrossWire repository, which is also the repository the catalog prefers when a module is offered more than once.

## Findings that shaped the code

- **One module name, one row, whatever the type.** IBT's `TSK` is a Bible and CrossWire's `TSK` is a Commentary. A SWORD folder can hold only one module of a name, so the catalog keeps one row per name. CrossWire wins, as for Bibles.
- **Refusals come from the conf.** Every refusal above (numbering, `HREFCom`, devotional) can be read from the conf before anything is downloaded, so those rows are shown disabled with the reason. AbbottSmith's problem (lemma keys) can only be seen in the data. It stays enabled and fails with the Import report's message, the same as any other module that has no entries.
- **Old caches.** A catalog cached by v0.1 lists Bibles only. The cache now has a version, and an older cache is still shown (offline too) but refreshed when the Tab opens.
