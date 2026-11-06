# Topical indexes: live SWORD check (issue #148)

**Question:** Which topical indexes can the Topic Guide read, under what licence, and what do
their entries look like after SWORD Import?
**Date:** 2026-11-06, from a Linux host against the live CrossWire repository.
**How:** `curl` of `https://www.crosswire.org/ftpmirror/pub/sword/raw/mods.d.tar.gz` (every conf
grepped for `topical`), then each module's `packages/rawzip/<Module>.zip`, imported with
`obs-cli import` (the app's SWORD Import) and read back with `obs-cli inspect`.

## What the catalog has

Exactly two confs in the main CrossWire repository carry `LCSH=Bible--Indexes, Topical.`:

| Module | Driver, markup | Version | Licence in conf | Text source | rawzip sha256 (2026-11-06) | Import |
|---|---|---|---|---|---|---|
| `Nave` (Nave's Topical Bible) | zLD, TEI | 3.0 (2021-02-28) | Public Domain | `https://ccel.org/ccel/n/nave/bible.xml` | `52d9b7cd…c945f0` (1,300,879 B) | 5,320 entries, 77,975 Bible references |
| `Torrey` (R. A. Torrey's New Topical Textbook) | RawLD, ThML | 1.3 | Public Domain | `http://www.bf.org/` | `9db6e3f1…4cb7c80` (422,465 B) | 628 entries, 21,247 Bible references |

Both are Free to share by the licence allowlists and both download from CrossWire itself. Nave's
conf names a ccel.org file as its source, so (as in [open-catalogue.md](open-catalogue.md) §5)
it is not a Starter library or Open catalogue candidate; Get Resources lists it as CrossWire
distributes it. Torrey is now Featured beside it.

## Entry shapes

- **Nave (TEI):** `<lb/>→ OF ENEMIES <ref osisRef="Exod.23.4">Ex 23:4</ref>, …` for each subtopic,
  then `<list><item>…</item></list>` for its items (`Esau forgives Jacob Ge 33:4,11`) and its
  cross-references (`<item>See <ref target="Nave:SIN">SIN</ref>, FORGIVENESS OF</item>`).
  SWORD Import used to drop `<list>`/`<item>` (14,707 tags), running the items into one line; it
  now keeps each `<item>` as a list paragraph and drops nothing. 638 entries cite no Passage and
  only send the reader elsewhere (`PARDON` → `See SIN, FORGIVENESS OF`; `IMPORTUNITY` → `See
  PRAYER`): the Topic Guide's synonyms. One source error is kept as it is: FORGIVENESS's
  `<ref osisRef="Phlm.1.1">Phm 1</ref>:10-18` targets Phlm 1:1.
- **Torrey (ThML):** `- Subtopic<br /><scripRef>Lu 23:34</scripRef><br /><br />. Item<br />…`: a
  label line, then its references on the next line. Forgiveness is under `FORGIVENESS OF
  INJURIES`; prayer under `PRAYER` and `PRAYER, ANSWERS TO`, `PRAYER, INTERCESSORY`, ….
- **Dictionaries:** Easton has no `Forgiveness` but `Forgiveness of sin`; ISBE has
  `FORGIVENESS`; Smith has neither. So Dictionary articles also look under longer headwords.

## Telling a topical index from a Dictionary by content

References per word, over entries of 30 words or more: Nave median 0.24 (10th percentile 0.13),
Torrey 0.12 (0.10), Easton 0.05 (90th percentile 0.13), Smith 0.03 (0.08). Torrey and the
reference-heavy tenth of Easton overlap, so a content rule would misfile entries; ADR 0009 names
the topical indexes by resource id instead.

## Result on the real modules

`OBS_TOPIC_LIBRARY=<Nave, Torrey, Easton, Smith, ISBE> cargo test -p obs-engine
real_library_forgiveness -- --ignored --nocapture`: the chooser for "forgiveness" lists
Forgiveness (ISBE, Nave), Forgiveness of injuries (Torrey), Forgiveness of sin (Easton) and
Pardon (see Sin, Forgiveness of). Nave's FORGIVENESS reads as OF ENEMIES (30 Passages, see
ENEMY), INSTANCES OF with 8 items, and OF SINS (92 Passages through SIN › FORGIVENESS OF);
Torrey's as 23 subtopics and items. Key Passages lead with Matt 18:21-35, Matt 6:14, Matt 6:15,
Mark 11:25, Eph 4:32 and Col 3:13. Directory and chooser: ~275 ms cold.
