# Library Tool visual evidence (#46)

Real `openbible` windows, captured on macOS with the app's `OBS_SHOT` framebuffer
callback (1280 × 800). Only conversion was PPM → PNG with ffmpeg; no compositing
or retouching. **No screen recording (no recorder on this host).**

| Preview | State checked |
| --- | --- |
| [Preferred order after relaunch](library-reordered-relaunched.png) | WEB was dragged from last to first. The app exited, then launched again with the same User database. WEB remains first and ★. No move command or pointer input runs in the relaunch capture. |
| [Combined filters](library-combined-filters.png) | Header text `test` AND selected English chip leave only WEB. Text alone also matches HEB; English alone also matches KJV. |
| [Remove confirmation](library-remove-confirmation.png) | Remove identifies WEB, its `.obr` deletion and zero referring Annotations; Remove and Cancel are visible. This capture does not confirm deletion. The engine and UI regression tests cover cancellation and confirmed deletion. |
| [About Tab](library-about.png) | Keyless About · KJV shows manifest fields, SPDX licence, verbatim pipeline-rendered Attribution, changes, source format, Annotation count and actions. |

The existing headless shell harness drives pointer/keyboard input and checks order,
★, filtering, About and confirmation/deletion. These real-window previews complement
those assertions; they are not kittest image-diff baselines.

## Fixtures

An isolated `.preview-data/library/` and `.preview-data/user.db` were used, never the
Mac user's library or User database. These are tiny test Resources, not new Starter
sources or release artifacts:

- KJV: `obs-cli write-sample .preview-data/library/kjv.obr` (the repository's public-domain
  `store::sample_resource`, unchanged).
- WEB: `obs-cli import crates/formats/tests/assets/usfm/web/3JN.usfm -o
  .preview-data/library/web.obr --id web-preview --title 'World English Bible (test)'
  --abbrev WEB --lang en` (existing test fixture and its Attribution).
- HEB: `obs-cli import .preview-data/GEN.usfm -o .preview-data/library/hebrew.obr
  --id hebrew-preview --title 'Hebrew Bible (test)' --abbrev HEB --lang hbo --rtl`.
  `GEN.usfm` contains the same mixed Hebrew/English text as `tests::hebrew_doc`:

```usfm
\id GEN
\h Genesis
\c 1
\p
\v 1 בְּרֵאשִׁ֖ית בָּרָ֣א (created) אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם
\v 2 וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ
```

The imported fixtures have no licence notice in their input, so their rows honestly
show `LicenseRef-unknown`. About uses the KJV sample's recorded licence/Attribution.

## Capture recipe

Build `openbible` in release mode on the graphical Mac, after copying this worktree
and the generated fixtures there (exclude `target` and `.git`). All builds for these
captures were run through the lane's shared `cargo-slot`, including a shell `cargo`
function that forwarded the Mac build over SSH while holding the local slot.

Set `OBS_LIBRARY` and `OBS_USER_DB` to absolute paths to the isolated fixture folder
and a fresh User database. Each launch uses `OBS_SHOT=<absolute-output>.ppm` and
`OBS_SHOT_FRAME=100`, exits after capture, and saves Last session normally.

`OBS_SCRIPT='2 view.theme {"value":"light"}'` sets the first launch's theme. The
first launch also uses this `OBS_INPUT` generator to drag WEB's handle above HEB
and widen the Library Panel. Coordinates are app-local logical points; the window
starts at the app's default 1280 × 800 size.

```python
import json
mods = dict(alt=False, ctrl=False, shift=False, mac_cmd=False, command=False)
def move(frame, x, y):
    return [frame, {"PointerMoved": dict(x=x, y=y)}]
def button(frame, x, y, pressed):
    return [frame, {"PointerButton": dict(pos=dict(x=x, y=y), button="Primary",
                                        pressed=pressed, modifiers=mods)}]
def click(frame, x, y):
    return [move(frame, x, y), button(frame + 1, x, y, True),
            button(frame + 2, x, y, False)]

events = [move(10, 876, 276), button(11, 876, 276, True)]
events += [move(12 + i, 876, 276 - i * 9) for i in range(1, 11)]
events += [button(24, 876, 186, False)]
events += [move(30, 850, 400), button(31, 850, 400, True)]
events += [move(32 + i, 850 - i * 20, 400) for i in range(1, 11)]
events += [button(44, 650, 400, False)]
print(json.dumps(events))  # OBS_INPUT for setup launch

# OBS_INPUT for combined-filter launch, after the relaunch capture:
print(json.dumps(click(10, 710, 98) + [[15, {"Text": "test"}]]
                 + click(20, 700, 146)))
```

Subsequent launches, all with the same fixture folder and User database:

1. **Relaunch:** no `OBS_INPUT` or `OBS_SCRIPT`; capture
   `library-reordered-relaunched.ppm`.
2. **Combined filters:** use the second generated `OBS_INPUT`, no `OBS_SCRIPT`;
   capture `library-combined-filters.ppm`.
3. **Remove:** unset `OBS_INPUT`; `OBS_SCRIPT='4 library.remove {"id":"web-preview"}'`;
   capture `library-remove-confirmation.ppm`.
4. **About:** `OBS_SCRIPT='4 resource.about {"id":"kjv-sample"}'`;
   capture `library-about.ppm`.

Copy the PPMs back, then for each image:

```sh
ffmpeg -y -i <capture>.ppm docs/previews/<capture>.png
```

Capturing exposed missing-glyph boxes for ★ and ≡ in the default proportional
font. The UI now enables egui's bundled monochrome symbol fallback for ★ and
uses the bundled monospace face for ≡. The Library grouping test asserts both
fonts contain their glyphs. No host/system fonts are required.
