# Note text is CommonMark, shown line by line, and plain-text Notes were escaped once

A Note's text is stored in the existing `annotations.body` column as **Note formatting**: CommonMark, read by a CommonMark parser (pulldown-cmark, no extensions). Headings, bulleted and numbered lists, block quotes, bold, italic, links (a target with spaces, such as a Bible reference, in `<…>`) and `\` escapes follow the spec, numbering included; code spans and blocks show their text, and raw HTML shows as typed (it is never run). One function (`obs_model::parse_note_text`) gives each line of the text what it shows, and serves the Notes Tool's cards, the Note editor's colouring and toolbar, Notes search and the card filter. The app draws it with its own text pipeline (ADR 0004), never a web view.

What is shown is laid out **per line of the text**: a line break inside a paragraph (a CommonMark soft line break) is a line break where the Note is shown, as it was when Notes were plain text, and each blank line is a gap. The HTML export renders soft breaks as `<br />` for the same reason.

Exports never re-write a Note's Markdown from a parse. The Markdown export is the text byte for byte as typed, never transformed or wrapped (no `\` added for line breaks: in a shortcut or collapsed reference link the text is the label, and a `\` there stops it matching its definition); only a closing line follows a code fence or HTML block the Note leaves open. The HTML export is pulldown-cmark's HTML of the text.

Plain text is not always the same CommonMark: a line like `# not a heading`, `- item`, `1. step`, `*stars*` or `    indented` would show differently. So the User database migration 10 → 11 runs `escape_note_text` over every Note: a Note that does not already show exactly as typed gets, on each line, what CommonMark would read as markup backslash-escaped and its leading and trailing spaces written as `&#32;`; every other Note is left byte for byte. `modified_at` is not touched, and `notes_fts` is rebuilt from the words the Notes show.

## Considered Options

- **Store plain text and treat it as Markdown with no migration** (what #186 first assumed): the Notes with those lines would change how they look, against the acceptance that old Notes look identical.
- **A per-Note format flag** (plain or formatted): every reader of `body` would need both paths for ever, and a Note would switch format the first time it was edited.
- **Rich text (an attributed-runs or HTML column)**: needs a WYSIWYG editor egui does not have, and makes export, search and the undo of edits harder.
- **A line-based Markdown subset with our own parser and exporter** (this PR's first design): every line read on its own, and the exports re-wrote the parse as CommonMark. Review after review found the re-written Markdown read differently from the Note (emphasis next to emphasis, link targets with `&amp;`, list numbers), because two dialects had to be kept in step by hand.
- **CommonMark, soft line breaks shown as breaks (chosen):** one spec, one parser, and exports that are the text itself.

## Consequences

- Escaping changes stored text, so it cannot be undone by a later build: a Note that held `# x` now holds `\# x` and shows `# x` in its editor with the backslash. That is the irreversible part. The pre-migration copy the User database takes before any migration keeps the old file.
- CommonMark's rules apply as typed: a numbered list counts on from its first number, a nested item goes under its parent's text (two columns under `- `, three under `1. `), a line after a list item or quote without a blank line continues it, and a link target with spaces needs `<…>`. The Note editor's toolbar writes it that way.
- A Note's Markdown in an export is not moved under the Annotation's heading: its `#` is the export's `#`. The HTML export moves Note headings two levels down.
- A Markdown reader shows a line break inside a paragraph of a Note as a space, as CommonMark says, not as the line break the app and the HTML export show; the Markdown export is the Note's own text, not a rendering of it.
- A link reference definition (`[x]: url`) in one Note also defines `[x]` for the Notes after it in a Markdown export.
- New CommonMark extensions (tables, strikethrough, task lists) must not be turned on without a migration that escapes what they would newly read.
