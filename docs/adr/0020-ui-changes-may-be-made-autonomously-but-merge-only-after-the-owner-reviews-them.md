# UI changes may be made autonomously, but merge only after the owner reviews them

Much of the work on the app is done by coding agents working on their own: they pick up an
issue, change the code, run the checks and open a pull request. For most of the code the checks
say whether a change is right. For the interface they cannot: the Screenshot gallery (ADR 0019)
says that the look changed, not whether the change is good, and an agent re-blessing the goldens
makes the gallery pass by definition. We decided:

1. **Any UI change may be made autonomously.** An agent may change the chrome, layouts,
   design tokens and shared components (ADR 0015), icons (ADR 0012), themes (ADR 0018), copy,
   dialogs, menus and keyboard behaviour, and re-bless the gallery goldens, without asking first.
2. **No UI change merges until the owner (@inhesrom) has reviewed it.** A pull request that
   changes what a user sees or how they interact with it waits for the owner's approving review,
   whatever its checks say and whoever or whatever opened it. An agent never merges one itself,
   and never enables auto-merge on one.
3. **The pull request shows the change.** It says which surfaces changed and includes before
   and after images (the gallery's changed goldens, or preview images under `docs/previews/`), so
   the review can be done from the pull request.
4. **When in doubt, it is a UI change.** A change whose effect on the interface is unclear (a
   refactor of shared components, a dependency bump of egui or egui_tiles, a font change) is
   treated as one.

## Considered Options

- **Ask before every UI change:** the owner keeps control, but every agent run stalls on a
  question, and a change is easier to judge when it can be seen than when it is described.
- **Let the checks decide, as for other code:** fast, but the gallery and the a11y matrix only
  catch accidents and contrast failures; whether a layout reads well or a control is where a user
  looks for it is a judgement no check makes.
- **Review only large UI changes:** "large" needs a rule an agent can apply, and small changes
  (a spacing token, a label) are the ones that slip through unnoticed and add up.

## Consequences

- UI pull requests queue for the owner; agents keep working on other issues meanwhile, and two
  pull requests touching the same goldens rebase as ADR 0019 describes.
- Branch protection on `master` should require an approving review from the owner for UI
  changes (a CODEOWNERS entry for `apps/openbible/`, `crates/ui-egui/`, `crates/render/`,
  `crates/fonts/` and `docs/previews/`).
- Changes with no effect on the interface (the engine, the store, the text pipeline's data,
  tests, CI) are not held by this decision.
