# The right-click menu (#142)

Right-clicking text is the main way from what you read to a study of it. The menu has two
columns: on the left, what to act on (its **targets**); on the right, the actions for the
target chosen. Hovering a target chooses it, so the action is one more click.

| Target | Shown as | Actions |
|---|---|---|
| Selection | “loved” | the active palette's Highlight styles, Highlight styles…, Note…, Copy, Search this, Look up in Dictionary, Word Study… (English-word chooser, up to 3 words) |
| Verse | John 3:16 | Passage Guide, Exegetical Guide, Cross-references, Text Comparison, Copy with reference, Add Note on verse |
| Lemma | ἀγαπάω | Word Study, Open in preferred Lexicon, Pin in Word Info, Search Bible for ἀγαπάω |
| Strong's number | G25 | Word Study, Open in preferred Lexicon, Pin in Word Info, Search Bible for G25 |
| Morphology | plain English | Copy, Pin in Word Info |

The target chosen when the menu opens is the Tagged word's Lemma (else its Strong's number),
else the selection, else the Verse. A right-click outside the selection selects the word
clicked (as before #142), so the Selection target is that word.

Each opening replaces what the menu offered before; a right-click with nothing to act on (blank
Text Comparison space, a Library-mode Search row, a Search pane without results) opens no menu.

## Where it opens

- **Reading panes** (`shell/context_menu.rs`, `State::reading_menu`): Bibles, Commentaries,
  Lexicons and Dictionaries. A Strong's link in a Lexicon or Commentary is a Tagged word; a
  Scripture reference under the pointer is the Verse target (its first Passage). While the
  worker is still translating the reference, the Verse row shows the reference as written with
  its actions disabled, and the open menu fills it in when the targets arrive (never the Verse
  around the reference meanwhile); a reference without targets is no Verse target. A
  Commentary's Verse is the section's Passage. The Verse
  label uses the Bible's own numbering (`RefStyle::Short`); an Unmapped verse keeps Copy and
  Add Note but not the Standard-keyed tools.
- **Text Comparison** Verse blocks (`ComparisonOutput::context`): the block's Verse; on a row
  with text, Copy with reference copies that row's Bible with the reference in **its own
  numbering** (HNUM's Gen 32:1 under the Standard Gen 31:55 copies as “Genesis 32:1 (HNUM)”;
  a verse bridge as “Genesis 1:1-2”).
- **Search** result rows in Bible mode (`SearchOutput::context`): the row's Verse; Copy with
  reference copies the hit under the pointer (else the row's first) in its Bible's numbering.
- **Keyboard**: Shift+F10 or the Menu key, in the focused reading pane (not while a text field
  has focus or the Note editor is open), at the word under the pointer (which it selects, like
  a right-click) or else at the selection. With neither (the keyboard alone, #182) it selects
  the Verse at the top of the view and acts on that. Tab moves through the menu; moving onto a
  target chooses it, as hovering does (a pointer resting on another target takes it back only
  once it moves), and Enter runs an action. Esc closes it.

## Commands

Every action is an engine command, so the command palette and the Command box reach the same
things. Ctrl/Cmd held when clicking sends `new_tab: true` (`split: true` for `search.run`, so a
new Search Tab opens instead of replacing the open one's query).

- `word_study.open {strongs, new_tab?}` — Word Study (moves the focused or most recent Word Study).
- `word_study.choose {words, new_tab?}` — the English-word chooser for an untagged word (a
  Strong's number studies that number).
- `lexicon.open {strongs, new_tab?}` — the first Lexicon in Preferred order with an entry for
  the number (an extended number falls back to its base), through
  `ResourceStore::lookup_strongs`; the most recent Tab of that Lexicon in the opener's Link
  set (from an unlinked Tab, an unlinked one) moves, else one opens and joins the opener's Link
  set. A Tab of it in another Link set is left alone: moving it would move that set.
- `dictionary.lookup {text, new_tab?}` — the first Dictionary in Preferred order with that
  Headword (normalized; aliases count), the same way.
- `guide.passage_guide`, `tool.cross_references`, `tool.text_comparison` take `passage`
  (Standard versification) and `new_tab`: the most recent Tab of that kind in the focused
  Tab's Link set (or, from an unlinked Tab, an unlinked one) moves to the Passage, else a new
  one opens there and joins the Link set; the Link set follows as after any jump.
- `search.run` — Search this (a phrase when several words; Bible mode in a Bible, Library mode
  in that Resource otherwise), and Search Bible for a Lemma (`lemma:`) or Strong's number
  (`strong:`) in the Bible clicked. A Lemma that an English Bible borrows from a Lexicon
  (KJV+Strong's tags numbers only) is searched by its Strong's number, which that Bible can
  answer.
- `annotation.highlight` / `annotation.note` — the Selection's styles and Note…; Add Note on
  verse selects the Verse in a Bible (a Passage anchor with its Text range) or anchors on the
  `passage` elsewhere.
- Pin in Word Info pins the word (`WordInfo::pin`) and opens Word Info (`tool.word_info`).

Tests: `crates/ui-egui/src/context_menu_tests.rs` (every target type, keyboard, Text
Comparison and Search rows) and `crates/engine/src/lookups_tests.rs` (the commands).
