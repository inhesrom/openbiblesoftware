# Fonts: Reading fonts and the Interface font (#138)

View → Fonts (`view.fonts`) opens the Fonts window. Each slot — Latin text, Greek text,
Hebrew text, Interface — is a Bundled font or a font installed on the computer, chosen with
`view.font {slot, family, style}` (an empty family is the Bundled font). See ADR 0004's #138
amendment for the design.

| Piece | Where |
|---|---|
| Choices by name, persisted in the `view` preference, missing-font notice flag | `obs-engine` `appearance.rs` (`FontSlot`, `FontChoice`, `FontChoices`), `Session::font_missing` / `font_found` |
| Installed fonts (enumerate, read bytes) | `apps/openbible/src/fonts.rs` (`FontdbFonts`, `fontdb`), behind `obs_ui_egui::SystemFonts` |
| Loading choices, the Fonts window, coverage notes, previews | `obs-ui-egui` `shell/fonts_window.rs` |
| Interface font in egui's `FontDefinitions` | `obs-ui-egui` `theme::install_fonts` |
| Runtime faces, cmap coverage, `smcp` detection | `obs-fonts` `face.rs` (`load_user_face`, `Face::covers`, `Face::has_small_caps`), `reading.rs` (`ReadingFaces`, `script_coverage`) |
| Per-cluster fallback, synthetic small caps, numbers in the Latin font | `obs-layout` `present.rs` (`push_text`, `push_run`, `SYNTHETIC_SMALL_CAPS`) |

Scripts: Hebrew and Greek characters use their own Reading font; every other letter uses the
Latin one. Spaces, digits and punctuation stay with the text around them (at the start of a
text, with the first letter), but like letters they are drawn in the Bundled font when the
chosen face lacks them. Verse and chapter numbers use the Latin font's bold face.

Coverage: a Reading font with none of its script's letters is flagged "This font has no Hebrew
letters; Ezra SIL will be used."; one missing some accents or marks is flagged with a count.
Either way the Bundled font draws what is missing, cluster by cluster.

Outlines: glyphs are painted from outlines only (`glyf`, CFF or CFF2), in the glyph atlas and
in egui. A font without them — a bitmap-only font (`EBDT`, `CBDT` or `sbix` alone) — maps
letters in its cmap yet would paint nothing, so `load_user_face` refuses it (`has_outlines`)
and the slot keeps its Bundled font, with the notice "… cannot be drawn (no outline glyphs, or
not a readable font) …" once and "Cannot be drawn (no outline glyphs)" in the Fonts window. The
font list still shows such families: telling them apart means reading every installed file.

Tests: `obs-fonts` `tests.rs` (loading, coverage, `smcp`), `obs-layout`
`reading_font_tests.rs` (fallback, including leading punctuation, synthetic small caps), `obs-engine` `workspace_tests.rs`
(`font_choices_are_saved_by_name_and_survive_a_restart`), `obs-ui-egui` `fonts_tests.rs`
(every pane changes, Hebrew fallback, restart and missing font, Interface font, a font
without outlines refused and the text still painted) and
`apps/openbible` `fonts.rs` (fontdb listing and loading, and this computer's fonts).
