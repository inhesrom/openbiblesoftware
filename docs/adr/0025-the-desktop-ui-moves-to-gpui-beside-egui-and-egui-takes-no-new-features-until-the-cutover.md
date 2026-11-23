# The desktop UI moves to GPUI beside egui, and egui takes no new features until the cutover

A throwaway prototype compared a four-Panel shell drawn with GPUI (Zed's UI framework) with
the egui app, running the same scripted scenario. The prototype is `prototype/gpui-shell` on
the branch `prototype-gpui-instead-of-egui`; its `REPORT.md` has the results. On the owner's
desktop (Intel Arc B390, Wayland, 120 Hz):
- GPUI missed the refresh on 3 % of scroll frames; egui missed it on 30 %.
- GPUI used about a quarter of the CPU per frame: 2.0 ms against 7.6 ms.

egui's own UI pass was fast. Most of its time went into eframe's OpenGL frame. GPUI's own text
system cannot draw our Resource text: it does not wrap right-to-left paragraphs, and it breaks
Greek words mid-word. The owner chose to move the desktop UI to GPUI and keep our text
pipeline (ADR 0004). The plan is the map #371 and its tickets. We decided:

1. **GPUI is built beside egui, on `master`.** A new `crates/ui-gpui` at layer 6 lands one
   surface at a time beside `ui-egui`, which keeps shipping until the cutover. There is no
   long-lived migration branch.
2. **A cargo feature picks the UI.** `cargo build -p openbible --features gpui` builds the GPUI
   app; the default build is egui until the cutover. Each binary holds one UI, so releases are
   unchanged until then. CI builds and tests both.
3. **egui is frozen for new UI features.** Once this ADR merges:
   - Nothing new starts in `ui-egui`.
   - UI pull requests already open may finish and merge. The surfaces they add get port
     tickets on the map, so the cutover waits for them: Books and printed pages (#351, #352),
     the historic-Book collections (#365, #366, #368), searching the Christian Open Library
     (#363) and Sync conflicts (#402).

   The engine, the data crates and mobile are not frozen. A feature can land below the UI now,
   and its desktop UI is built in GPUI.
4. **egui still gets bug fixes.** A fix to a surface that GPUI already has goes into both UIs
   in the same pull request, or the pull request files the GPUI bug and links it.
5. **`obs-reading` is the shared reading core.** The crate at layer 5, from the mobile plan's
   S01 (#359), holds the reading model egui, GPUI and the mobile shells all use. Ticket #376
   moves only what S01 did not, and waits for #359. If #359 stalls, #376 takes the move over.
6. **The no-networking rule keeps its purpose.** The app makes network requests only through
   its own transport (ADR 0017, Get Resources). Two ways to keep that, in order of preference:
   - Prefer a GPUI build without its HTTP client, through a feature, a `[patch]` or a small
     fork.
   - If #373 finds none, exactly GPUI's HTTP crates are allowed, in `ui-gpui` only, listed by
     name in `xtask layers`. A check that `ui-gpui` never constructs an HTTP client comes with
     the exception. Any other networking crate, in any library, still fails.
7. **The wasm32 check stops at the UI.** Every crate below layer 6, `obs-reading` included,
   must still build for `wasm32-unknown-unknown`. `ui-gpui` is exempt. A web build is out of
   scope; if one comes, it decides its own UI.
8. **MPL-2.0 is accepted for unmodified dependencies.** GPUI brings MPL-2.0 crates
   (`option-ext`, and `dwrote` on Windows). `about.toml` accepts MPL-2.0, and those crates are
   listed in Help → Credits like the others. If we ever patch an MPL file, the patch is
   published.
9. **The cutover bar.** GPUI becomes the default in #401 only when all of these hold:
   - every Screenshot-gallery surface is drawn by GPUI and approved by the owner (#399);
   - the accessibility audit is green, and a screen reader works on macOS, Windows and Linux
     (#398);
   - the #19 targets are met at the display's refresh rate, measured with the windowed bench
     (#384);
   - automation and the update end-to-end test pass, and a release candidate installs and
     runs on all three systems (#400);
   - the owner has used a GPUI release candidate as their everyday app for two weeks, with no
     blocking bug open.
10. **No fallback after the cutover.** The cutover release is GPUI only; the previous egui
    release stays downloadable from GitHub. `ui-egui` and its dependencies are deleted from
    `master` once the cutover release is tagged. An emergency egui fix branches from the last
    egui tag.
11. **Port pull requests are reviewed side by side.** Every port pull request carries the
    `ui-review` label. For each surface it ports, it shows egui and GPUI side by side (the
    gallery images) and lists the differences that are intended. `crates/ui-gpui/` is a UI path
    under ADR 0020. The merge train is unchanged.
12. **The migration can stop after G0.** It stops, and the egui freeze lifts, if any of these
    holds:
    - our laid-out text cannot be painted through GPUI within the #19 targets (#372);
    - no GPUI build has AccessKit working on all three systems (#373);
    - GPUI surfaces cannot be tested and screenshotted headlessly (#374).

    The owner makes the call, and the reasons are added to this ADR.

## Considered Options

- **Coexistence:**
  - *A long-lived migration branch:* `master` stays egui-only, but the branch must follow every
    egui change for months and ends in one huge review.
  - *Rewriting `ui-egui` in place:* the fastest to finish, but nothing ships until it is done.
- **Choosing the UI:**
  - *A runtime switch with both UIs compiled in:* would allow an opt-in preview, at twice the
    UI build time and binary size for everyone.
  - *A second app binary:* duplicates the app glue for no gain over a feature.
- **UI work during the move:**
  - *A per-surface parity rule, egui still open for features:* every new egui surface would
    lengthen the port. The freeze fixes the target the port has to reach.
  - *New features built only in GPUI:* nothing could ship until the cutover.
- **Freeze start:**
  - *Parking the open UI pull requests:* wastes finished work.
  - *Freezing only once G1 is done:* keeps growing the surface the port must cover.
- **Gates:**
  - *No networking exception ever:* could force a GPUI fork for a client the app never calls.
  - *Exempting the UI layer from the networking rule:* leaves it to review.
  - *Requiring wasm32 for `ui-gpui`:* would narrow the choice of GPUI for a web build that is
    out of scope.
  - *Keeping the licence list permissive:* would mean patching `dirs` and `font-kit` out of
    GPUI.
- **After the cutover:**
  - *Shipping an egui download for one release, or keeping egui buildable for a cycle:* keeps
    two UIs alive after the point of the move.
- **No stop points:** with egui frozen, a failure found in G0 would keep it frozen for nothing.
- **Checking first whether eframe's wgpu renderer closes the gap:** cheaper than a move. The
  owner chose not to wait for it.

## Consequences

- **ADR 0005** (upstream egui with egui_tiles) is superseded by this ADR at the cutover.
  Until then it still describes the shipping UI.
- **ADR 0004:** this ADR amends it. GPUI draws the chrome, and #372 decides how our laid-out
  text is painted through GPUI.
- **ADR 0008:** Floating windows become GPUI windows of the one app, still drawing Tabs of the
  one Workspace and Session (#388).
- **ADR 0020:** `crates/ui-gpui/` is added to its list of UI paths.
- **ADR 0019:** the gallery's goldens are redrawn by GPUI; #374 decides how.
- **Port tickets:** every surface added to egui by a pull request open at the freeze gets a
  port ticket on #371 that #398, #399 and #401 wait on.
- **CI cost:** while both UIs exist, CI builds two UI crates, and the Linux image needs GPUI's
  system libraries.
- **Desktop UI features:** they wait for GPUI. Planning should expect no new desktop UI until
  G3 (the GPUI Workspace) has landed.
