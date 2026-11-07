# Highlight styles are rows the user edits, and their Page-tone colours are derived

v0.1 fixed Highlight styles at five (#18 rule 9: yellow, green, blue and pink backgrounds and a red underline), hard-coded in the app with a hand-tuned colour per Page tone. #181 reverses that part of #18: the user adds, renames, reorders and deletes styles and groups them into **Highlight palettes**. What #18 kept stays: an Annotation stores only its style's id, so the five built-in ids (`yellow`, `green`, `blue`, `pink`, `red_underline`) are seeded as rows of a **Standard** palette by the User database migration and every Highlight made before keeps its look.

Styles and palettes are rows of the annotation tables (`highlight_styles`, `highlight_palettes`), written through the same row Changes as Notebooks and Annotations. Deleting a style that Highlights use moves them to a style the user chooses and deletes the style in one undo step, so undo brings back both; the store refuses to delete a style a Highlight still uses, so no Highlight is ever left without its style and none is ever dropped (ADR 0006).

A style stores one colour, a look (background, underline, box or text colour) and, per Page tone, an optional adjusted colour. Without an adjustment the colour for a tone is **derived** when it is drawn: a background fades toward the page until every text colour of the tone reads on it at WCAG AA (4.5:1), and an underline, box, text colour or margin mark deepens toward the ink until it reaches 3:1 (4.5:1 for text) against the page (#21). An adjustment that fails the same check is refused by the styles window and, should one reach the database another way, is drawn derived instead.

## Considered Options

- **Keep the fixed set and add more built-ins:** no data change, but students colour-coding themes still run out, and it is the gap #18 recorded.
- **Styles as a JSON preference:** no migration, but deletions could not share an undo step with the Highlights they move, and nothing in the store could stop a style in use from vanishing.
- **Rows in the annotation tables (chosen):** one more migration, and undo, the in-use check and the "never dropped" rule come from the machinery Annotations already have.
- **The user picks a colour per tone:** the most control, but three colours to choose per style, and a light-paper colour would be unreadable in Dark until the user fixed it. Deriving with an optional adjustment keeps every tone readable by default.

## Consequences

- Colours of Highlights are no longer part of the Page-tone palettes: those hold the page, inks and overlays, and a style's colour comes from the style and the tone at draw time. Switching tone still never re-lays out text.
- The Selection keys for Highlights become slots: Ctrl+1…9 highlight with the active palette's first nine styles. Keys the user had rebound for the old five styles move to slots 1–5. (Since #215 the slots' default keys are Alt+1…9: Ctrl+1…9 go to the Panels. Keys the user set for a slot are kept.)
- Exported palettes (a small JSON file) carry names, colours, looks and adjustments, never ids; importing one always makes a new palette, so it can never rewrite styles that Highlights use.
