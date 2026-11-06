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
A scheme for numbering chapters and verses. Every Bible has its own, which is the numbering it shows; it need not match any named tradition exactly. Different traditions number some passages differently (English Ps 3:1 is Hebrew Ps 3:2).
_Avoid_: Verse system, Canon (for numbering)

**Standard versification**:
The one Versification that the app treats as shared: KJV numbering of the 66-book Protestant canon, with a title slot for each Psalm superscription. Link sets, Annotations, Cross-reference sets and every Passage-keyed Resource other than a Bible speak it, so they work across Bibles that number differently.
_Avoid_: Canonical versification, Master versification

**Verse map**:
A correspondence between the Verses of one Bible and the Verses of the Standard versification. Each Bible has its own Verse map, made when the Bible is imported. It works in whole Verses: one Verse may correspond to several, and some Verses correspond to none.
_Avoid_: Mapping table, Versification map

**Unmapped verse**:
A Verse in a Bible that has no counterpart in the Standard versification, such as deuterocanonical text. It can be read, but Link sets do not follow it and Annotations on it use a Text-range anchor.
_Avoid_: Extra verse, Orphan verse

**Absent verse**:
A Verse of the Standard versification that a particular Bible does not contain (Mt 17:21 in some modern texts). That Bible shows a gap where it would be.
_Avoid_: Missing verse, Omitted verse

**Passage**:
A contiguous range of Verses. It may cross chapters or Bible books. One Verse or a whole chapter is still a Passage.
_Avoid_: Range, Reference (for the location itself)

**Bible reference**:
The written form of one or more Passages, such as "Jn 3:16-18; Rom 5:8", as typed by the user or found in a Resource's text. It parses to a Passage list. It is read in the Versification of where it is written: a Bible's own numbering when typed into that Bible's Tab or found in that Bible's text, the Standard versification everywhere else.
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
The set of Resources bundled with the installer, available offline from first launch. They are added to the library on the first launch; an installer without them offers to download them under Library → Starter library…, which also lists each one and adds back any that are missing.
_Avoid_: Default library, Base package

**Removed Starter Resource**:
A Starter library Resource the user removed from the library. It is remembered in the User database and never added back silently; Library → Starter library… lists it as Removed, with Add back.
_Avoid_: Deleted resource, Hidden resource

**Import**:
Adding a Resource to the library from a local file in an open format (USFM or SWORD in v0.1). Every Import produces an Import report.
_Avoid_: Install, Download

**Get Resources**:
The Tab that lists the Bibles, Lexicons, Commentaries and Dictionaries offered by online SWORD repositories, Featured ones first, each one click from download and Import. "Get Bibles" is the same Tab filtered to Bibles.
_Avoid_: Store, Shop, Module manager, Install (for the whole flow)

**Update available**:
What Get Resources shows on a downloaded Resource when its SWORD repository now offers a newer `Version` of the module it was downloaded as; the Library menu counts them. Update (one Resource) or Update all downloads it and imports it as the Resource's next Resource revision. Updates are checked when the catalog is fetched, and nothing is downloaded until the user clicks. When the repositories cannot be reached, or only a catalog saved by an earlier run of the app is known, the Tab says updates could not be checked or when they last were, never that the Resources are up to date. Starter library Resources are updated with the Starter library, not here.
_Avoid_: Upgrade, New version (as the name of the offer)

**SWORD copy**:
The SWORD module files a downloaded Resource was imported from, kept in the app's own SWORD folder. Updating a Resource replaces its SWORD copy; removing the Resource deletes it.
_Avoid_: Module cache, Download folder

**Import report**:
The summary shown after an Import: what was brought in, which markup was dropped, how many Unmapped and Absent verses the Bible has, and any licence information that was missing.
_Avoid_: Import log, Conversion warnings

**Search index**:
A cache built from one Resource so that it can be searched. It is derived data, never shipped as part of the Resource and never user data, and it can be rebuilt at any time.
_Avoid_: Index (unqualified), Search database

**Search scope**:
What a search runs over: a Passage list and a set of Resources (or Notebooks for notes). It is chosen beside the query, not written inside it.
_Avoid_: Filter, Range (for the scope)

**Resource revision**:
A corrected release of the same Resource, such as a typo fix in a Bible. It replaces the earlier revision and keeps the Resource's identity, so its Preferred order and Annotations carry over. A downloaded Resource gets one through Update available. A different edition is a different Resource, not a revision.
_Avoid_: Update, Version (for a correction)

**Attribution**:
The license and credit text that a Resource requires to be shown, displayed in that Resource's About view.
_Avoid_: Credits, Copyright notice

**Credits**:
The app-level list under Help → Credits: every Starter library Resource, the data built into the app, the bundled fonts and the third-party software, each with its licence. A Resource's own Attribution stays in its About view.
_Avoid_: Acknowledgements, About (for this list)

**Change record**:
The published `CHANGES-<id>.md` stating the changes made to a CC BY Starter library source, linked from that Resource's About view.
_Avoid_: Changelog, Diff

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
A Tab that is neither a Resource nor a Guide. In v0.1 the Tools are Text Comparison, Word Info, Search, and Notes; Cross-references, Favorites and History came later.
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

**Favorite**:
A Current location the user has starred, kept with its Tab kind (a Bible or other Resource, a Guide, Text Comparison, Cross-references or a Search) so that opening it brings back that kind of Tab at that location. It may sit in a Favorites folder.
_Avoid_: Bookmark, Pin, Saved location

**Favorites folder**:
A named group of Favorites. A Favorite is in at most one; folders exist while they hold Favorites.
_Avoid_: Collection, Category, Notebook (for Favorites)

**History**:
The Tool listing the Visits of every Tab, newest first and grouped by day. It keeps 90 days (at most 5,000 Visits), can be filtered, and can be cleared. It is not a Tab's Back/Forward history.
_Avoid_: Recent, Recently viewed, Journal

**Visit**:
One entry in History: a Tab's Current location just after the Tab was opened or jumped (typing a reference, following a link, Back or Forward, running a search). Scrolling and following a Link set make no Visit.
_Avoid_: History entry, Hit

**Last session**:
The Workspace as the user left it, restored automatically at the next launch. It is not a Layout.
_Avoid_: Autosave layout

**Text size**:
How large Resource text is drawn in every Tab, in points. View → Larger text, Smaller text and Reset text size change it (Ctrl/Cmd with +, = or numpad +; − or numpad −; 0). It never changes the Interface size.
_Avoid_: Zoom, Font size

**Interface size**:
One zoom on the whole app window: menus, Tab bars, headers and dialogs grow or shrink, with Resource text scaled along on top of its Text size (View → Interface size: Smaller, Default, Larger). It is kept in the User database. No Text size key changes it.
_Avoid_: Zoom, UI scale, DPI

**Reading font**:
The font Resource text of one script is drawn in: one each for Latin, Greek and Hebrew, chosen in View → Fonts. It is a Bundled font by default, or a font installed on the computer, stored by family and style name. Characters a Reading font lacks are drawn in the Bundled font for that script.
_Avoid_: Text font, Body font, Resource font

**Interface font**:
The font of the app's own menus, labels and buttons, chosen in View → Fonts. Resource text never uses it.
_Avoid_: UI font, System font, Chrome font

**Bundled font**:
A font shipped inside the app: Gentium Plus for Latin and Greek, Ezra SIL for Hebrew. It is the default Reading font and the fallback when a chosen font is missing or lacks a character.
_Avoid_: Default font, Built-in font

### Guides

**Guide**:
A Tab that takes one Key and shows a report made of ordered Guide sections. Its Guide arrangement is remembered for each kind of Guide.
_Avoid_: Report, Dashboard

**Guide arrangement**:
The order of a Guide's sections, which ones are collapsed, and which are hidden. It is remembered for each kind of Guide, not for each Tab.
_Avoid_: Guide layout, Section settings

**Guide section**:
One block of a Guide that gathers related results (for example commentaries or Cross-references) for the Guide's Key.
_Avoid_: Panel, Card, Widget

**Passage Guide**:
The Guide whose Key is a Passage. Its sections in v0.1 are Commentaries, Cross-references, Key words, and People & places.
_Avoid_: Exegetical Guide

**Word Study**:
The Guide whose Key is a Strong's number. Its sections in v0.1 are Lexicon entries, Translated as, Occurrences, and Forms.
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

**Show in text**:
A Notebook's setting for whether its Annotations are drawn in Resource text. Annotations in a Notebook with it off are still kept and listed; they are just not drawn.
_Avoid_: Visible, Filter, Layer

**Anchor**:
What ties an Annotation to the text. It is either a Passage anchor or a Text-range anchor.
_Avoid_: Attachment, Link

**Passage anchor**:
An Anchor on a Passage in the Standard versification, used for Bible Annotations on any text that is not an Unmapped verse. It shows in every Bible that contains the Passage. It may also carry a Text range in the Bible where it was made, so that one Bible can render it word by word.
_Avoid_: Reference anchor, Verse anchor

**Text-range anchor**:
An Anchor on a Text range in one specific Resource, shown only in that Resource. Used for non-Bible Resources and for Unmapped verses in a Bible.
_Avoid_: Offset anchor, Location anchor

**Orphaned annotation**:
An Annotation whose Text-range anchor could no longer be found after its Resource changed. It is kept with the text it quoted until the user re-attaches it, detaches it, or deletes it. Nothing is ever orphaned silently or deleted automatically.
_Avoid_: Broken note, Lost highlight

**Needs attention**:
The Notes Tool's list of Orphaned annotations, where each can be re-attached to selected text, kept without an anchor (Notes only) or deleted. It shows only when there are some.
_Avoid_: Broken links, Errors

**User database**:
The single local store of everything the user has made or chosen: Notebooks, Annotations, Layouts, Last session, Preferred order, Guide arrangements, Removed Starter Resources, Favorites, History and preferences. Resources and Search indexes are never part of it.
_Avoid_: Profile, User library, Settings file
