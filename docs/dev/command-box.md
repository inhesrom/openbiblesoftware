# The Command box (#139)

The box at the right of the menu bar. It replaced the toolbar's Search field, which only ran
`search.run`: typing now shows a dropdown of suggestions, best first, and Enter runs the
highlighted one. Searching is still there for any text: the list holds at most eight
suggestions, and when it is full Search keeps the last slot.

## What text suggests

The engine decides (`crates/engine/src/command_box.rs`,
`Session::command_box_suggestions`); the UI (`crates/ui-egui/src/shell/command_box.rs`) only
draws the list and runs `command_box.run`. Ranks, highest first (equal ranks keep this order):

| Rank | Text | Suggestion | Runs |
|---|---|---|---|
| 100 | a Layout's name (any case) | Layout | `layout.restore` |
| 95 | `pg`, `guide`, `passage guide` + reference | Passage Guide | `guide.passage_guide {passage}` |
| 95 | `eg`, `exegetical guide` + reference | Exegetical Guide | `guide.exegetical_guide {passage}` |
| 95 | `compare`, `tc`, `text comparison` + reference | Text Comparison | `tool.text_comparison {passage}` |
| 95 | `xref`, `xrefs`, `cross-references` + reference | Cross-references | `tool.cross_references {passage}` |
| 95 | `ws`, `study`, `word study` + Strong's number, Lemma, transliteration or English words | Word Study | `word_study.open` / `word_study.choose` |
| 95 | `tg`, `topic`, `topic guide` + text (#148) | Topic Guide's chooser for it | `guide.topic_guide {text}` |
| 95 | `open` + Resource | Open … | as below |
| 95 | `search` + text | Search | `search.run` |
| 95 | `scroll speed` + 0.25 to 4 (`scroll speed 2`, `1.5x`, `0.5×`, `150%`, #293) | Scroll speed 2× · Setting · Motion | `view.scroll_speed {value}` |
| 92 | a Resource's abbreviation or title (any case) | Open Bible, Open Lexicon … | focuses its most recent Tab, else `resource.open` |
| 90 | a Bible reference: `jn 3:16`, `jn 3 16`, `1 jn 4 8`, `Rom 5:8; Jn 3:16` (first Passage) | Go to · *Bible* | see below |
| 90 | a Strong's number: `G26`, `h1254a` | Word Study | `word_study.open` |
| 85 | a command's label: `notes`, `history`, `close tab` (any command the box lists, #226) | Command (with its key: `Command · Ctrl+W`) | that command, on the focused Tab if it needs one |
| 80 | a Lemma in a Lexicon (`ἀγάπη`, `αγαπη`), or a transliteration in a Lexicon's transliteration index (`agape`, `chesed`) | Word Study (`ἀγάπη · G26`) | `word_study.open` |
| 65 | words starting a Resource's abbreviation or title (`extended strongs`) | Open … | as above |
| 60 | words starting a Layout's name | Layout | `layout.restore` |
| 56 | a Saved search's name or query (`Session::saved_searches_matching`, #179) | Saved search | `search.open_saved` |
| 55 | a Favorite's label or folder (`Session::favorites_matching`, #155) | Favorite | `favorite.open` |
| 50 | words starting a command's label | Command | that command |
| 10 | anything | Search for “…” | `search.run` |
| 5 | one to three words of letters that nothing above understood | Word Study (English) | `word_study.choose` |

References are read in the Standard versification, as everywhere outside a Bible's own Tab
(CONTEXT.md, Bible reference). A reference goes to the focused Tab if it is a Bible, else to
the most recent Bible Tab, else opens the first Bible in Preferred order there
(`Session::show_passage`, as the Notes Tool does). The Link set follows, as after any jump.

A Lemma's Strong's number is the Lexicon entry's own: `ResourceStore::keys_of_unit` lists the
Keys of the entry found by `lookup_key(Key::Lemma)`, and the non-alias Strong's number wins
(TBESH's `H1254A` rather than the base `H1254` it is also listed under). Lexicons are asked in
Preferred order, one suggestion per base number. On the Starter library this takes a few ms
per keystroke (release build: `agape` 14 ms, `chesed` 25 ms, references and numbers < 1 ms);
the UI asks only when the text changes. Without files (wasm) there are no Lemma suggestions.

## All commands (#226)

`>` at the start of the text lists commands only: with nothing after it, every command the
box can run (`Session::listed_commands`: those needing no parameter but the focused Tab,
except `SCRIPTING_COMMANDS` and `IN_PLACE_COMMANDS`), menu commands first in menu order, the
disabled ones left out; with words after it, the commands whose label matches (whole label,
then words starting the label's words, then words anywhere in it). There is no limit of
eight: the dropdown scrolls, ↑/↓ keeping the highlight in view. Each shows its Keyboard
shortcut, as the platform writes it (`⌘W` on macOS, #262). **Help → All commands** (`app.commands`, Ctrl+Shift+P) puts the keyboard in the
box with `>` typed and the caret after it (`Effect::ListCommands`). A `>` text is not kept as
a recent entry. `docs/dev/menus.md` says where every command is.

A command run from the box behaves as from its menu, button or key:

- The box empties before the command runs, so one that puts the keyboard back in it (All
  commands, Command box) keeps what it put there; a failed run puts the text back.
- The command run is the one the dropdown showed: the UI passes its id (`command`), since the
  suggestions, found again, leave out disabled commands and one made possible meanwhile (an
  Undo) would shift them.
- Undo restore (`data.undo_restore`) is guarded as everywhere else (#217): the UI asks
  `Session::command_box_choice` which command the chosen suggestion runs and, for a restore
  or its undo, saves the Notes being written first (holding the undo back if the database
  refuses them) and forgets the Notes Tab's drafts after.
- Word Info's Copy (`word_info.copy`) hands its text to the UI as `Effect::CopyText`, which
  writes the clipboard, so the box, a key and the Copy button all copy.

## Keyboard and mouse

- **Ctrl+L** (`command_box.focus`, Cmd+L on macOS) puts the keyboard in the box and selects its
  text, from anywhere, also from another text field.
- **↑ / ↓** move the highlight; **Enter** runs it; **Alt+Enter** runs it in a new Tab
  (`new_tab: true`: a new Tab of the Bible, Guide or Tool rather than moving the open one; a new
  Tab joins the opener's Link set, which then follows it); **Esc** leaves the box.
- A click on a suggestion runs it (Alt held: in a new Tab).
- After running, the box empties and the keyboard goes back to the Workspace. A suggestion
  that fails (no Bible in the library, say) leaves the text and shows why.

## Recent entries

Every text run from the box is remembered, newest first, at most 20, in the User database
(preference `command-box.recent`, a JSON array, like the Search Tool's recent queries). The
empty box lists up to 8 as **Recent**; running one runs that text's best suggestion again and
moves it to the top.

## Commands

- `command_box.focus` — Ctrl+L; the UI focuses the box (`Effect::FocusCommandBox`).
- `command_box.suggest {text?}` — the suggestions as JSON (`kind`, `label`, `detail`); an
  empty text gives the recent entries. `obs-cli run command_box.suggest text=agape`.
- `command_box.run {text, choice?, new_tab?}` — run suggestion `choice` (default 0, the best)
  for `text` and remember the text.

Tests: `crates/engine/src/command_box_tests.rs` (the parser and ranking, each action, recent
entries across a restart, on a real `.obr` library with the Starter Lexicons' excerpts) and
`crates/ui-egui/src/command_box_tests.rs` (Ctrl+L, typing, ↑/↓, Enter, Alt+Enter, Esc, click,
recent entries after a relaunch).
