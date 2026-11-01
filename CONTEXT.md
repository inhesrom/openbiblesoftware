# Open Bible Software

An offline desktop Bible study app for the serious lay student. Users read, compare, annotate and search a local library of open resources.

## Language

### Scripture locations

**Bible book**:
One book of the biblical canon, such as Genesis or John.
_Avoid_: Book (unqualified)

**Verse**:
One point in Scripture, given by Bible book, chapter and verse number in a particular Versification.

**Versification**:
A scheme for numbering chapters and verses. Every Bible declares one. Different traditions number some passages differently (English Ps 3:1 is Hebrew Ps 3:2).
_Avoid_: Verse system, Canon (for numbering)

**Standard versification**:
The one Versification that the app treats as shared. Link sets, Annotations and Cross-reference sets speak it, so they work across Bibles that number differently.
_Avoid_: Canonical versification, Master versification

**Verse map**:
A correspondence between the Verses of one Versification and the Verses of the Standard versification.
_Avoid_: Mapping table, Versification map

**Passage**:
A contiguous range of Verses. It may cross chapters or Bible books. One Verse or a whole chapter is still a Passage.
_Avoid_: Range, Reference (for the location itself)

**Bible reference**:
The written form of one or more Passages, such as "Jn 3:16-18; Rom 5:8", as typed by the user or found in a Resource's text. It parses to a Passage list.
_Avoid_: Ref, Citation

**Passage list**:
An ordered list of Passages, used where a single location is not required (for example a search scope).

**Cross-reference**:
A link from one Passage to a related Passage, supplied by a Cross-reference set.
_Avoid_: Xref, Related verse, Reference (unqualified)

### Original-language words

**Tagged word**:
A word in a Bible's text that carries original-language data: a Strong's number and, where available, a Lemma, Morphology and Gloss.
_Avoid_: Token, Interlinear word

**Lemma**:
The dictionary form of a Hebrew, Aramaic or Greek word, which all of its inflected forms share.
_Avoid_: Root, Lexeme

**Strong's number**:
The numeric identifier for a Lemma in the Strong's system (such as H1254 or G5485), including extended forms that tell homographs apart.
_Avoid_: Strong's code, Strongs ID

**Morphology**:
The grammatical parsing of one Tagged word (part of speech, tense, case, and so on), shown to the user in plain English.
_Avoid_: Parsing code, Morph tag (in user-facing text)

**Gloss**:
A short English meaning for a Lemma or a Tagged word.
_Avoid_: Definition, Translation

### Library

**Resource**:
One item in the user's library: a text that can be imported and licensed on its own, with its own metadata. One Resource is one edition as shipped: a multi-volume set is one Resource, and differently tagged editions of the same text (e.g. KJV with and without Strong's) are separate Resources. Datasets that only tools read (cross-references, people/places) are Resources too.
_Avoid_: Book, Module, Work, Title

**Resource type**:
The kind of a Resource. There are six: Bible, Commentary, Lexicon, Dictionary, Cross-reference set, and People & places index. Language is a separate attribute, so a Hebrew or Greek original text is a Bible.
_Avoid_: Category, Genre, Original-language text (as a type)

**Bible**:
A Resource holding the text of Scripture in one language and edition, organized by Bible book, chapter and Verse.
_Avoid_: Translation, Version (as type names)

**Commentary**:
A Resource of explanatory prose whose sections are keyed to Passages.

**Lexicon**:
A Resource of entries keyed by Lemma or Strong's number.
_Avoid_: Dictionary (for lemma-keyed works)

**Dictionary**:
A Resource of entries keyed by an English headword (a topic, person, place or thing).
_Avoid_: Encyclopedia, Lexicon (for headword-keyed works)

**Cross-reference set**:
A Resource made of Cross-references.

**People & places index**:
A Resource that identifies people and places and lists the Passages that mention them.

**Key**:
One value that a Resource is organized by and can be opened at, such as a Passage, "G5485", or "Abraham".
_Avoid_: Datatype value, Index entry

**Key scheme**:
The kind of a Key. There are four: Passage, Strong's number, Lemma, and Headword. Each Resource declares the Key schemes it is organized by.
_Avoid_: Datatype, Index kind, Reference scheme

**Text range**:
A span of text inside one specific Resource, given by position rather than by Key. It is only meaningful for that Resource.
_Avoid_: Offset, Selection (for the stored span)

**Preferred order**:
The user's ranking of Resources within one Resource type. The first Resource in the ranking is used for lookups, and guides follow the ranking.
_Avoid_: Priority, Top Bibles, Favorites

**Starter library**:
The set of Resources bundled with the installer, available offline from first launch.
_Avoid_: Default library, Base package

**Import**:
Adding a Resource to the library from a local file in an open format (USFM or SWORD in v0.1).
_Avoid_: Install, Download

**Attribution**:
The license and credit text that a Resource requires to be shown, displayed in that Resource's About view.
_Avoid_: Credits, Copyright notice

### Workspace

**Workspace**:
The live arrangement of Panels and Tabs in the app window.
_Avoid_: Layout (for the live arrangement), Desktop

**Panel**:
A dockable, resizable region of the Workspace that holds one or more Tabs.
_Avoid_: Pane, Window, Dock

**Tab**:
One open thing in a Panel: a Resource, a Guide, or a Tool.
_Avoid_: View, Document

**Tool**:
A Tab that is neither a Resource nor a Guide. In v0.1 the Tools are Text Comparison, Word Info, Search, and Notes.
_Avoid_: Utility, Widget

**Word Info**:
The Tool that shows a Tagged word's Lemma, Strong's number, Morphology and Gloss when the user hovers over or clicks the word.
_Avoid_: Info panel, Information tool

**Text Comparison**:
The Tool that shows one Passage in several Bibles as Verse-aligned rows. It can optionally mark the wording that differs from a base Bible.
_Avoid_: Compare, Diff view

**Current location**:
The Key that a Tab is showing right now.
_Avoid_: Position, Cursor

**Link set**:
A lettered group of Tabs (A, B, C…) that share a Current location. When one Tab navigates, every other member whose Resource has that Key scheme follows, translating through Verse maps. A Tab belongs to at most one Link set.
_Avoid_: Sync group, Linked panels

**Layout**:
A named, saved snapshot of the Workspace, including its Link sets, that the user can restore.
_Avoid_: Workspace (for the saved snapshot), Preset, Arrangement

**Last session**:
The Workspace as the user left it, restored automatically at the next launch. It is not a Layout.
_Avoid_: Autosave layout

### Guides

**Guide**:
A Tab that takes one Key and shows a report made of ordered Guide sections. The order of the sections is remembered for each kind of Guide.
_Avoid_: Report, Dashboard

**Guide section**:
One block of a Guide that gathers related results (for example commentaries or Cross-references) for the Guide's Key.
_Avoid_: Panel, Card, Widget

**Passage Guide**:
The Guide whose Key is a Passage.
_Avoid_: Exegetical Guide

**Word Study**:
The Guide whose Key is a Strong's number.
_Avoid_: Bible Word Study, Lexical study

### Annotations

**Annotation**:
Something the user attaches to the text: a Note or a Highlight.
_Avoid_: Markup, Mark

**Note**:
An Annotation holding the user's own text. It has either one Anchor or none.
_Avoid_: Comment, Memo

**Highlight**:
An Annotation that shows a span of text in a Highlight style. It has exactly one Anchor.
_Avoid_: Mark, Markup

**Highlight style**:
One look from the small fixed set used to render a Highlight.
_Avoid_: Color, Palette

**Notebook**:
A named container for Annotations. Every Annotation belongs to exactly one Notebook, and a default Notebook exists.
_Avoid_: Folder, Collection, Tag

**Anchor**:
What ties an Annotation to the text. It is either a Passage anchor or a Text-range anchor.
_Avoid_: Attachment, Link

**Passage anchor**:
An Anchor on a Passage in the Standard versification, used for Bible Annotations. It shows in every Bible that contains the Passage. It may also carry a Text range in the Bible where it was made, so that one Bible can render it word by word.
_Avoid_: Reference anchor, Verse anchor

**Text-range anchor**:
An Anchor on a Text range in one specific non-Bible Resource, shown only in that Resource.
_Avoid_: Offset anchor, Location anchor
