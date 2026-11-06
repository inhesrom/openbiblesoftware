# Note text is a line-based Markdown subset, and plain-text Notes were escaped once

A Note's text is stored in the existing `annotations.body` column as **Note formatting**: a small Markdown subset (headings, bulleted and numbered lists, block quotes, bold, italic and links whose target is a Bible reference or a URL; `\` escapes). It is read **line by line**: every line break is a line break where the Note is shown, as it was when Notes were plain text, and a blank line is a gap. One parser (`obs_model::parse_note_text`) serves the Notes Tool's cards, the Note editor's colouring, Notes search, the card filter and Notes export. The app draws it with its own text pipeline (ADR 0004), never a web view.

Plain text is not always the same Markdown: a line like `# not a heading`, `- item`, `1. step` or `*stars*` would now show formatted. So the User database migration 9 → 10 runs `escape_note_text` over every Note: a line that would read as formatting gets its markup characters backslash-escaped, and every other Note is left byte for byte. `modified_at` is not touched, and `notes_fts` is rebuilt from the words the Notes show.

## Considered Options

- **Store plain text and treat it as Markdown with no migration** (what #186 first assumed): the few Notes with those lines would change how they look, against the acceptance that old Notes look identical.
- **A per-Note format flag** (plain or formatted): every reader of `body` would need both paths for ever, and a Note would switch format the first time it was edited.
- **Rich text (an attributed-runs or HTML column)**: needs a WYSIWYG editor egui does not have, and makes export, search and the undo of edits harder; Markdown is already what Notes export writes.
- **CommonMark exactly** (soft line breaks, lazy continuation, setext headings, code, HTML): a typed line break would join lines, as it does not today, and much of the syntax has no use in a Note.
- **Line-based subset, one-time escaping (chosen).**

## Consequences

- Escaping changes stored text, so it cannot be undone by a later build: a Note that held `# x` now holds `\# x` and shows `# x` in its editor with the backslash. That is the irreversible part. The pre-migration copy the User database takes before any migration keeps the old file.
- Markdown export writes the parsed lines back as CommonMark: hard line breaks between lines, Note headings two levels down, and a blank line where CommonMark would otherwise join a list or quote to the text after it. A trailing `\` (CommonMark's hard break) is markup here too, so exports read back.
- New syntax added later must not change how existing Note text shows, or it needs its own escaping migration.
