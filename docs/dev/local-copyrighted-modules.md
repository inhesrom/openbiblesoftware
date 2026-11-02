# Testing with copyrighted modules locally (NASB95)

Some Bibles we want to test against, such as the NASB 1995, are copyrighted. CrossWire may distribute them, but we may not. This page explains how a developer gets one onto their own machine, and the rules that keep it out of anything we share.

## Rules

1. **Never commit the module.** That includes git LFS, test fixtures and example data. The module, the `.obr` Resource file built from it on Import, and its Search index are all copies of the text.
2. **Never bundle it in a build.** Release, nightly or "experimental" builds that anyone else receives carry only the Starter library. An experimental build may *Import* NASB95 from a path on the tester's machine. It must never contain it.
3. **Never put it in CI.** No caches, artifacts or uploaded logs containing the module, its converted output, or more than short quoted excerpts of its text.
4. **Each tester downloads their own copy** from CrossWire, as described below. Don't pass the zip around.
5. **Shared, committed fixtures use open texts only.** Use public-domain or CC modules for those (KJV, ASV, WEB, BSB; see `docs/research/open-content-inventory.md`).

Rules 2 and 4 follow from the licence line in the module's own conf file:

```
DistributionLicense=Copyrighted; Permission to distribute granted to CrossWire
```

Lockman's public terms allow no more than 1,000 verses to be "stored in an electronic retrieval system" without written permission. Whether a user's own Import (convert to `.obr` and build a Search index) is acceptable is still an open question for Lockman (contact listed by CrossWire: pike@lockman.org). Until they answer, keep NASB95 to personal development testing.

## Getting NASB95

CrossWire hosts Lockman's modules in a separate repository. They are unencrypted (no unlock key), use the zText format, and carry Strong's numbers.

| Item | Location |
|---|---|
| Package zip | <https://ftp.crosswire.org/ftpmirror/pub/sword/lockmanpackages/NASB1995.zip> |
| Conf files (raw repo) | <https://ftp.crosswire.org/ftpmirror/pub/sword/lockmanraw/mods.d/> |

Module names: `NASB1995` is the 1995 text. `NASB` is now the **2020** edition, not the 1995 one.

Unpack it outside the worktree, or into the gitignored `local-modules/` folder:

```sh
mkdir -p ~/bible-modules/sword
cd ~/bible-modules/sword
curl -LO https://ftp.crosswire.org/ftpmirror/pub/sword/lockmanpackages/NASB1995.zip
unzip NASB1995.zip   # gives mods.d/nasb1995.conf and modules/texts/ztext/nasb1995/
```

You can also install it with Xiphos or BibleTime (add the "Lockman Foundation" repository) and point at that SWORD folder instead.

## Pointing the app and tests at it

Code does not exist yet. These are the conventions to follow when it does:

- **`OBS_LOCAL_SWORD_DIR`**: a SWORD root (the folder holding `mods.d/` and `modules/`). Integration tests that need a real copyrighted module read this variable and **skip, not fail**, when it is unset or the module is missing. Tests may check structure (verse counts, Verse map, Strong's spans, Import report) but must not snapshot or print whole passages of the text.
- **Experimental builds** Import from a path the user picks or from `OBS_LOCAL_SWORD_DIR`. They never embed module files with `include_bytes!`, build scripts, or installer payloads.
- **Converted output** (`.obr`, Search index) goes to the user's normal library and cache directories, or to a temp directory in tests. Never write it under the worktree, except inside `local-modules/`.

## Related

- `docs/research/open-bible-format-survey.md`: SWORD format and the `sword-modules` crate
- `docs/research/open-content-inventory.md`: what we *can* ship
- ADR 0003: Resource files written on Import
