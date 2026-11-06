# Copy verses, Notes export and printing (#154)

Formatted verses for handouts and studies, Notes out of the app, and printing. Terms are in
[CONTEXT.md](../../CONTEXT.md): **Copy verses**, **Copy preset**, **Notes export**,
**Attribution**.

| Layer | Piece | Role |
|---|---|---|
| `obs-model` | `manifest.rs` | `Manifest::requires_attribution`: a Resource with an Attribution whose licence is not public domain or CC0. `attribution_optional` evaluates the SPDX expression (`AND` binds tighter than `OR`, parentheses group): credit is optional only when some way of satisfying it uses public-domain/CC0 licences alone, so `LicenseRef-PD OR CC-BY-3.0` is optional and `(CC0-1.0 OR MIT) AND CC-BY-4.0` is not. An unknown licence or an expression that does not parse requires it. |
| `obs-engine` | `copy_verses.rs` | `CopyOptions`, `CopyPreset`, `copy_verses` → `CopiedText` rendered as plain text, HTML or Markdown; `html_document` (print pages); commands `tool.copy_verses`, `copy.option`, `copy.preset_save`, `copy.preset_apply`, `copy.preset_delete`. Options and presets live in the User database preferences `copy.options` and `copy.presets`. |
| `obs-engine` | `notes_export.rs` | `Session::notes_export` (a Notebook, all Notebooks, or a Search Tab's Notes results) → `NotesExport` rendered as Markdown, HTML or a print page; command `notes.export` (`format`, `notebook` or `tab`, optional `path`). |
| `obs-ui-egui` | `shell/copy_window.rs` | The Copy verses window: Passages, Bibles, preset, options, preview, Copy plain text / rich text / Markdown, Print…. `ReadingDoc` implements `CopiedBible`. The preview is Resource text, so it is drawn line by line through the text pipeline (ADR 0004: bidi, Reading fonts, glyph atlas), each line's direction from its first strong character (`obs_layout::starts_rtl`). The copy is made again when a chosen Bible's revision, content hash or file changes; a removed Bible is no longer chosen. |
| `obs-ui-egui` | `shell/export.rs` | The save dialog for Export (on its own thread), print pages written to `<temp>/openbible-print/` and opened in the browser. |
| `obs-ui-egui` | `rich_clipboard.rs` | `RichClipboard`: HTML plus its plain-text form. The desktop app (`apps/openbible/src/clipboard.rs`) implements it with `arboard`, which eframe already links; without one, rich text is copied as plain text. |

## Copy verses

Passages are typed as a Bible reference in the Standard versification and found in each Bible
through its Verse map, as Text Comparison does. Each Bible keeps its own verse numbers, and
each block's reference is in its own numbering (`Malachi 3:19-20 (HNUM)` for Standard Mal
4:1-2), like a plain Copy (#34). Blocks go Passage by Passage, one per Bible in the order the
Bibles were ticked. A Passage a Bible lacks gives no block. Verses come in the Bible's own
order: where its Verse map swaps verses (Standard Phil 1:16-17 is 1:17, 1:16 in a
Greek-numbered Bible) they are copied as that Bible prints them, under its own range.

| Option | Values |
|---|---|
| Reference | before or after the text (default after) |
| Verse numbers | on (default) or off; a chapter that starts inside a block shows `4:1` |
| Layout | paragraphs where the Bible starts them (default), or one verse per line |
| Quotation marks | “ ” around each block's text (default off) |
| Book names | full (`John 3:16`) or abbreviated (`Rom 8:28`, SBL abbreviations; John stays whole) |

How each format writes them:

| | Plain text | Markdown | Rich text (HTML) |
|---|---|---|---|
| Verse number | `16 ` | `**16** ` | `<sup>16</sup>&nbsp;` |
| Paragraphs | one per line | blank line between | `<p>` each |
| One verse per line | newline | `\` hard line break | `<br>` in one `<p>` |
| Reference | its own line | its own paragraph | `<p>` |
| Attribution | after a blank line | its own paragraph | `<p><small>` |

Verse text is copied as the Resource has it (white space collapsed), so the KJV's ¶ marks
stay. Markdown backslash-escapes the backslash, backtick, `*`, `_`, `[`, `]`, `<`, `>`, `#`,
`|`, `~` and an `&` that could start a character reference (`&amp;` stays literal), and at
the start of each line a block marker (`-`, `+`, `=`, the `.`/`)` of `1.`); HTML escapes `&`,
`<`, `>` and `"`.

Rich text goes on the clipboard as HTML with the plain text alongside, so word processors
paste it formatted and plain editors get the text.

## Notes export

```markdown
# Sermon prep

## Matthew 5:43 (KJV)

> love thy neighbour

*Highlight (yellow)*

## Romans 13:10 (KJV)

> Love worketh no ill to his neighbour

Love fulfils the law.\
\# not a heading

## No anchor

Unanchored thoughts.

---

Attribution of each quoted Resource that requires one.
```

- Each heading is the Notes Tool's reference: it reads back as a Bible reference in the
  Standard versification once the Bible abbreviation in parentheses is removed.
- The quote is the Text range's quoted text (white space collapsed) as a block quote.
- A Note's text keeps its lines: CommonMark's special characters are backslash-escaped,
  block markers at a line start (`-`, `+`, `=`, `1.`) are escaped, lines end in a hard line
  break, leading spaces are written `&#32;`, and a blank line stays a paragraph break.
  Trailing spaces on a line are dropped.
- A Notebook lists in Bible order; a Notes search result keeps its order (newest change
  first) and is titled `Notes matching “…”`.

HTML exports are a standalone page (`<h1>` title, `<h2>` per Annotation, `<blockquote>`,
`<p>` paragraphs with `<br>` line breaks, `<hr>` before the Attributions).

The Notes Tool's **Export…** menu (in the Notebook column, under + New notebook) exports the
Notebook chosen there (All and Needs attention export every Notebook); the Search Tool's Notes
mode has the same menu for its results.

## Print / Save as PDF

The app has no printing of its own. **Print…** in Copy verses and **Print / Save as PDF…** in
the Export menus write an HTML page (a print stylesheet, and a script that opens the print
dialog once loaded) to `<temp>/openbible-print/<name>.html` and open it in the system browser;
its print dialog prints it or saves it as a PDF. On the web build printing is not offered.

## Tests

- `crates/model/src/manifest.rs`:
  `only_resources_whose_licence_asks_for_credit_require_their_attribution` (grouped and
  malformed SPDX expressions included).
- `crates/engine/src/copy_verses_tests.rs`: Jn 3:16-18 in the KJV and BSB with a saved preset
  as exact Markdown and HTML; every option; escaping, including character references and
  line-start block markers with verse numbers and quotation marks off; Attribution for a CC BY
  Bible only; a Bible numbered differently; a Verse map that swaps verses; presets kept
  across a restart.
- `crates/engine/src/notes_export_tests.rs`: a Notebook exported to a `.md` file and read back
  (Passages, quotes, Note text with Markdown-special characters); HTML and print pages; a
  Notes search result; the Attribution of a quoted CC BY Resource.
- `crates/ui-egui/src/copy_export_tests.rs`: the window from Tools → Copy verses…, a preset
  saved and chosen again, the preview, the three Copy buttons (rich text through a test
  `RichClipboard`), Print…; a Hebrew Bible's preview laid out right to left in the Hebrew
  Reading font size by the text pipeline in every format; a chosen Bible updated and removed
  with the window open; the Notes Tool's Export → Markdown file and Print page.
- `crates/layout/src/tests.rs`: `a_line_reads_right_to_left_when_its_first_strong_character_does`.
