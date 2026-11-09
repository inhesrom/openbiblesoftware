# High contrast is a pair of Themes, and a Custom theme is a built-in base with the roles changed

#224 asks for a High contrast theme for light and dark (WCAG AAA text, thick focus rings, no colour-only cues) and a Theme editor whose result can be exported and imported as JSON. We decided:

1. **High contrast is two more Themes**, High contrast light and High contrast dark, beside Light, Dark and Follow system: one Theme choice, one id in the View preferences, one more chrome in `tokens` each (`HIGH_CONTRAST_LIGHT`, `HIGH_CONTRAST_DARK`). A chrome says whether it is high contrast (`Chrome::high_contrast`); when it is, the focus ring is heavy, egui's controls get a border that reads at 3:1, links are underlined at rest, and every Page tone is drawn from its own high-contrast palette (`Palette::*_HIGH_CONTRAST`) in which Highlight colours are derived to AAA (7:1) instead of AA. The a11y matrix (#182) draws every surface in both and requires AAA.
2. **One Custom theme**, a seventh Theme choice. It is stored as a built-in base and the colour roles changed from it (`CustomTheme { base, colors }`), not as a full set of colours. Its high-contrast behaviour comes from its base. Every edit in the Theme editor makes it the Theme, so the window is the preview.
3. **The theme file** (`openbible-theme`, version 1) is that same shape: `base` (a built-in Theme's id) and `colors` (role id → `#rrggbb`, only the changed roles). The role ids are the chrome's colour-role names (`shell`, `ink`, `lapis` …) and are a file format from now on. Import refuses an unknown format, a newer version, a base that is not built in, or a role it does not know; the User database keeps the theme in the same JSON (`custom-theme` preference).
4. **The engine keeps the choice, the UI the colours.** The engine owns `Theme`, `ThemeRole`, `CustomTheme`, the file and its commands; colours of the built-in themes and the contrast check (`theme::contrast_shorts`) stay in the UI, as for Page tones (#21).

## Considered Options

- **High contrast as a toggle over Light/Dark/Follow system:** it could follow the system's light and dark, but it is a second setting that changes what the first means, and a Custom theme would need a rule for which of the two wins. Two Theme choices keep one setting.
- **A Custom theme as every role's colour:** self-contained files, but a theme saved today would not pick up a later fix to its base's other colours, and the file would not say what it was made from. A base plus changes says both.
- **Several named Custom themes:** a list to manage for a feature most users set once; the file covers sharing and keeping alternatives.
- **Page colours in the Custom theme:** the Page tones and their Highlight derivation (ADR 0012) would need an editor of their own; Page tone stays a separate setting, and High contrast pages come with a High contrast base.

## Consequences

- The colour-role ids and the file format are public; renaming a role needs a file version 2 that maps the old id.
- A Custom theme the user makes unreadable is allowed; the editor warns with the ratio and what is needed, and Settings' Reset of Theme (Follow system) or any built-in is one click away.
- The a11y matrix runs twice as many theme × tone combinations.
