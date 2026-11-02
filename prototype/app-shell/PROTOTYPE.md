# App shell prototype (issue #13): THROWAWAY

## Polished-direction follow-up

**Question:** can the chosen panel-header shell look finished in egui, before considering a framework change?

Three polished directions were tried: **Study desk** (docked Bibles, commentary and Tools), **Reader's table** (two roomy Bibles plus a study drawer), and **Research ledger** (library navigator, docked panes and inspector). All kept per-Tab link chips, history and reference boxes.

**Verdict:** the user preferred **Study desk**. This prototype now opens only that direction; the switcher and the other two layouts were removed. Their screenshots remain in `screenshots/` as evidence.

The visual system uses cool page surfaces, lapis navigation, verdigris study accents, Gentium Plus display headings, and a vertical link thread on each Tab header. Performance details are hidden by default; enable them from **View** or with `OBS_DIAGNOSTICS=1`.

Screenshot smoke check: `OBS_SHOT='0;Gen 1:1;/tmp/shot.ppm'`. The first field is ignored. `OBS_SHOT_FRAME` delays capture when the compositor needs time to resize the window.

> **Question:** what should the main window feel like? This is a rough egui shell to react to before the UI gets specced. It isn't production code: there are no tests beyond a reference-parser smoke test, no persistence, and the error handling is minimal. It lives only on the `prototype/app-shell` branch.

## Verdict

The user tried the prototype hands-on and picked these:

- **Chrome:** variant **B, panel headers**. Each Tab has its own header strip with a link chip, back/forward and a reference box. This was the closest to Logos. A's global toolbar box and C's drawer and palette were not adopted.
- **Link sets:** followers track **continuously** while the user scrolls. A jump also flashes the target in every member.
- **Commentary gaps:** when a Commentary has nothing for the verse, it shows the **nearest earlier section**.
- **Word Info:** updates on **hover**, and keeps the last word after the pointer leaves.

## Run

```sh
prototype/app-shell/run.sh
```

The first run calls `prep_data.py`, which downloads about 50 MB of open sources into `.cache/` and writes Genesis extracts into `data/` and fonts into `assets/fonts/`. None of these are committed. After that, the script runs `cargo run --release`.

## What's here

- **Workspace:** an `egui_tiles` dock with tabs, splits, drag-to-dock, close (×), and a **+** button on each tab bar to open another Resource or Tool. Saved Layouts are kept in memory only (**Layouts** menu).
- **Link sets:** the lettered chip on each Tab (A/B/C/–). Click it, or right-click a tab title, to change it. When you scroll any linked Tab, the others follow continuously. A jump (reference box, a clicked cross-reference, a clicked commentary reference) also flashes the target verse in every member.
  - The Bible-to-Bible link goes through the **Verse map**. TAHOT shows Hebrew numbering: KJV Gen 31:55 lands on TAHOT 32:1, and TAHOT's own reference box says "Genesis 32:1".
  - A Commentary follows to the last section that is at or before the verse (JFB 31:52 for 31:55). Scrolling a Commentary drives the Bibles too.
- **Reference box:** accepts `Gen 3:15`, `gn 3.15`, `Genesis 22`, `2:4` (same book), or `5` (same chapter). Each Tab has its own back/forward history. References outside Genesis show a toast.
- **Tools:** *Cross-references* (OpenBible.info, ranked by votes, with KJV previews rendered by the pipeline) and *Word Info*. Word Info follows your hover over any Tagged word: the KJV gets its Strong's number and lemma, TAHOT gets the word, transliteration, gloss, lemma, Morphology in plain English, and the raw codes.
- **Popups:** hovering a † footnote shows the translator's note. Hovering a Scripture reference in a commentary shows the KJV text.
- **Text pipeline slice (ADR 0004):** all Resource text (reading panes, cross-reference previews, Hebrew lemmas in Word Info, popups) goes through `src/text.rs`. That means harfrust and skrifa shaping, UAX #9 bidi, UAX #14 line breaking, our own hit-testing, and vello_cpu rasterizing one texture per block. egui draws only the chrome plus the hover, selection and flash rects underneath the textures.
  - **Virtualization:** one block per text unit. Blocks that haven't been laid out yet use estimated heights. The scroll position is anchored as (block, offset), so a resize or font change keeps the same text at the top.
  - **Selection:** drag to select (it auto-scrolls past the edge), double-click selects a word, triple-click selects a verse. Ctrl+C copies **from the model**, with the reference and the version label.
  - **Text settings:** font size, Hebrew scale, line spacing and verse numbers (**Text** menu).
- **Status bar:** shows each link set's members and location, plus last-frame stats: layout and raster counts and milliseconds, block textures, and texture MB.

## Three chrome variants (bottom pill, or ← / → when no text field is focused)

| | Variant | Navigation | Where the Tools live |
|---|---|---|---|
| A | **Toolbar** | One global reference box, history and link chip that act on the *focused* Tab (Ctrl+L) | Docked tiles |
| B | **Panel headers** | Every Tab has its own header strip with a link chip, ‹ ›, a reference box and the title (Logos-like) | Docked tiles |
| C | **Focus + palette** | A centered location crumb that opens a command palette (Ctrl+L). There are no reference boxes | Bibles are tiled. Commentary, Cross-references and Word Info sit in a right **Study drawer** that follows the link set |

## What to react to

1. Global reference box (A), per-Tab headers (B), or crumb + palette (C)? Or a mix, such as B's headers plus C's palette?
2. Docked Tools (A/B) or a Study drawer (C)? Does the drawer make the link set feel implicit in a good way, or a confusing one?
3. Link sets: should followers track **continuously** while you scroll (as now), or only on jumps? Is the yellow flash on jump useful?
4. Should a Commentary that has nothing for the current verse show its nearest earlier section (as now), or an explicit "no comment on 31:55"?
5. Current location: is it right that it's the verse that *starts* on the top line, or else the verse containing the top line? Should a jump stay pinned to its target until you scroll?
6. Word Info on **hover** (it sticks after you move away) or on click?
7. How does reading feel: Gentium and Ezra SIL, the size of the Hebrew, grey verse numbers, † markers, LORD in small caps?

## Findings so far (measured on this machine)

- **The pipeline slice works.** Hebrew is laid out RTL with niqqud and cantillation (Ezra SIL through harfrust's Hebrew shaper). LTR verse numbers inside RTL text resolve through UAX #9. The KJV's `\nd` needs **ASCII lowercasing plus `smcp`**, because the source text is already `LORD`. Lowercasing ASCII keeps the byte offsets, so standoff spans stay valid.
- **Layout is cheap.** All of Genesis, 2,806 blocks across 4 Resources, lays out in about 0.9 s single-threaded, roughly 0.3 ms per block. Rasterizing a block takes 0.2–2 ms.
- **Texture memory is the real cost.** About 30 MB for 8–18 retained blocks at 1× DPI, which would be roughly 4× that at 2×. TAHOT paragraphs (פ/ס sections) reach **4,800 pt tall** at a 450 pt width, which is about 9.6k px at 2×, close to GPU texture limits. "One block = one paragraph" needs a cap (split long paragraphs into line-group tiles), a tighter retention window than ±3 viewports, or the glyph-atlas fallback from ADR 0004. This feeds the *performance targets* fog item.
- **Hebrew line height:** under the tallest-font-on-the-line rule, Ezra SIL's large ascent and descent make Hebrew lines visibly loose. We'll probably want per-script line-height tuning.
- **egui pitfall for `ui-egui`:** header widgets wider than the pane grow the Ui's `max_rect`, which made the reading body lay out wider than its pane. The fix is to pin the body to the pane rect before laying anything out.
- **Current location needs a rule in the spec** (see question 5). Without one, a jump to 31:55 reported 31:54, because the target line begins with the end of verse 54.

## Sources (downloaded by `prep_data.py`, not committed)

- KJV with Strong's: eBible `eng-kjv2006`, public domain.
- TAHOT: STEPBible, CC BY 4.0, STEP Bible (www.STEPBible.org). Only Genesis is extracted. Morpheme separators are removed and the grammar codes are expanded to English.
- JFB and MHCC: CrossWire SWORD modules, public domain.
- Cross-references: OpenBible.info, CC BY 4.0.
- Fonts: Gentium Plus 6.200 and Ezra SIL 2.51, both under the SIL OFL.
