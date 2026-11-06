# Hebrew & Greek: live SWORD catalog and licence check (issue #133)

**Question:** Which Hebrew and Greek Bibles do the SWORD repositories offer under the names #133 lists (OSMHB, WLC, SBLGNT, MorphGNT, TR, Byz, WHNU, a Septuagint), under what licence, which are tagged with Strong's numbers and Morphology, and does SWORD Import turn their tagging into Tagged words the Morphology expander reads?
**Date:** run from a Linux host against the live repositories while building #133.
**How:** `curl` of `https://crosswire.org/ftpmirror/pub/sword/masterRepoList.conf` and each repository's `mods.d.tar.gz` (CrossWire 428 confs, Beta 11, Attic 88, Xiphos 84, IBT 155, eBible.org 1530, STEP Bible 61, Wycliffe 44, Deutsche Bibelgesellschaft 1), read with a small script. The candidate packages (`packages/rawzip/NAME.zip` on CrossWire) were downloaded, their `<w>` attributes counted, and every distinct `robinson:` and `oshm:` code run through `obs_model::morph::parse` (kept as `crates/model/tests/assets/morph/sword-codes.tsv`). Then the ignored live test `live_every_featured_hebrew_and_greek_bible_downloads_and_imports_with_its_tags` (`apps/openbible/src/live_tests.rs`) downloaded every Featured module through the app's own transport, imported it, and read the Forms of G26 and H1254.

## Candidates

"S/M" is the tagging the conf declares: `OSISStrongs` (or `Feature=StrongsNumbers`) / `OSISMorph`.

| Module | Repository (version) | Lang | S/M | Morph scheme | Licence in conf | Featured |
|---|---|---|---|---|---|---|
| OSMHB | **CrossWire Attic only** (2.0, "morphology forthcoming") | he | S/– | – | Public Domain | no: retired; OSHB replaces it |
| OSHB | CrossWire (2.1.4, `Versification=MT`) | hbo | S/M | `oshm:` (OSHB codes) | Creative Commons: BY 4.0 | **yes** |
| WLC | CrossWire (2.0), Attic (1.8) | hbo | –/– | – | Public Domain | **yes** (untagged; no badges) |
| SBLGNT | CrossWire (1.3.1) | grc | –/– | – | Copyrighted; Free non-commercial distribution | **yes** (untagged) |
| MorphGNT | CrossWire (2.0.1, NRSV) | grc | S/M | `robinson:`, lemma `lemma.Strong:` | Copyrighted; Permission to distribute granted to CrossWire | **yes** |
| TR | CrossWire (3.2, KJV), Attic (2.1, Public Domain) | grc | S/M | `robinson:` | Creative Commons: BY-NC-SA 4.0 | **yes** |
| Byz | CrossWire (3.1, KJV), Attic (2.1, Public Domain) | grc | S/M | `robinson:`; a verb's Strong's tense-voice-mood number follows its number in `lemma` | Creative Commons: BY-NC-SA 4.0 | **yes** |
| WHNU | CrossWire (2.4.1), Attic (2.1) | grc | S/M | `robinson:` plus `strongsMorph:` | Creative Commons: BY-NC-SA 4.0 | **yes** |
| Tisch | CrossWire (2.5.1) | grc | S/M | `robinson:`, lemma `lemma.Strong:`/`lemma.ANLEX:` | Public Domain | **yes** (not in #133's list; the only Public Domain tagged Greek NT on CrossWire) |
| StatResGNT, SRKoine | CrossWire | grc | S/M | `packard:` | Creative Commons: BY 4.0 | no: Packard codes, which the expander does not read |
| LXX | CrossWire (3.2, `Versification=LXX`) | grc | S/M | `packard:` | **Copyrighted; Free non-commercial distribution** | no (see Septuagint) |
| LXX | CrossWire Attic (2.5) | grc | S/M | | Public Domain | no |
| LXX_th | STEP Bible | grc | S/M | | Public Domain | no |

Every Featured module is in CrossWire's main repository, which the catalog prefers anyway. The repository's TR, Byz and WHNU are the 3.x CC BY-NC-SA editions; their older Public Domain 2.1 editions are only in the Attic.

## Septuagint

Not featured. Rahlfs' edition is copyrighted by the Deutsche Bibelgesellschaft, and the CCAT morphological analysis that every tagged SWORD LXX derives from is distributed under a user declaration. CrossWire's main repository labels its LXX "Copyrighted; Free non-commercial distribution", while the Attic and STEP Bible copies say "Public Domain" for the same text. With the licence in doubt and its Packard codes unread by the Morphology expander, the Septuagint stays in "All Hebrew & Greek" only, with its own licence badge.

## Import and Morphology

| Module | Verses | Tagged words | Distinct codes | Unread codes | Import warnings |
|---|---|---|---|---|---|
| OSHB | 23213 | 300808 | 3466 | 0 | 0 |
| WLC | 23213 | 0 | 0 | – | 0 |
| MorphGNT | 7928 | 134738 | 596 | 0 | 0 |
| SBLGNT | 7925 | 0 | 0 | – | 0 |
| Tisch | 7895 | 137098 | 1035 | 0 | 0 |
| TR | 7957 | 140993 | 1064 | 0 | 0 |
| Byz | 7949 | 141112 | 1059 | 0 | 0 |
| WHNU | 7941 | 139732 | 1055 | 0 | 0 |

After the download, Word Study › Forms of G26 reads from MorphGNT ("noun, nom. fem. sing." ×36, "noun, acc. fem. sing." ×33, …) and of H1254 from OSHB ("Qal perfect, 3rd masc. sing." ×7, …).

## Findings that shaped the code

- **Import already read `strong:` and the morph code.** SWORD Import made a Tagged word from `lemma="strong:…"` and kept the `morph` code after its `robinson:`/`oshm:` prefix. Three shapes were wrong:
  - Byz follows 28,941 verbs' numbers with Strong's tense-voice-mood code (`lemma="strong:G1080 strong:G5656"`). That made a second, meaningless part (G5656) on each verb. Numbers G5625–G5999 (past the end of Strong's Greek dictionary, G5624) are now skipped.
  - TR and Byz have placeholders `strong:G` and `strong:G0`, each reported as an Import warning (276 in Byz, 13 in TR). They are now skipped silently.
  - WHNU adds `strongsMorph:` items to `morph`; they are no longer counted as Morphology codes.
  A lemma written in the text (`lemma.Strong:βίβλος`, MorphGNT and Tisch) is now kept as the Tagged word's Lemma.
- **The expander needed three shapes STEPBible's data never uses.** 83 of the 4685 distinct codes failed: OSHB's `x` for "not applicable" (`HPdxms`, a demonstrative has no person; `ANxxxa` in Dan 5:25), Tischendorf's possessive without the possessor's number (`S-1NSM`), and Robinson's imperatival participle mood `R` (`V-RPR-ASN`). All are read now.
- **OSHB words without a number** (`lemma="c/m"`, a preposition with a suffix; 5,977 words) carry no Strong's number and stay untagged text.
- **Tags from the conf.** CrossWire declares tagging in `GlobalOptionFilter` (`OSISStrongs`, `OSISMorph`); `Feature=StrongsNumbers` alone misses MorphGNT, whose conf has `OSISStrongs` but no such Feature, and there is no Feature for Morphology at all. The catalog reads both, so the badges are right before anything is downloaded.
