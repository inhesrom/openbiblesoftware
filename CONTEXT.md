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

**Interlinear lines**:
Short lines stacked under each Tagged word in a Bible Tab, toggled from the Tab header; the user picks which lines show, and the choice is kept per Tab and saved in Layouts. In an English Bible with Strong's numbers they form a **reverse interlinear** (Lemma, transliteration, Strong's number, Morphology, Gloss: the Hebrew or Greek behind each English word); in a Hebrew, Aramaic or Greek Bible they form an **interlinear** (Gloss, transliteration, Morphology, Strong's number under the original text). The lines are not text of the Resource: Annotations never anchor in them.
_Avoid_: Interlinear text, Interlinear Bible (it is the same Bible with lines under it), Glosses row

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

**Topical index**:
A Dictionary whose entries are Passage lists: each headword's subtopics ("Of enemies", "Instances of") with the Passages that bear on them, and "See" references to other topics. Nave's Topical Bible and Torrey's Topical Textbook are the two; the Topic Guide reads them. It is a Dictionary, not a seventh Resource type.
_Avoid_: Topical Bible (as the kind), Concordance, Topic list

**Cross-reference set**:
A Resource made of Cross-references.

**People & places index**:
A Resource that identifies people and places and lists the Passages that mention them. Its places may carry Place locations (the Starter library's OpenBible.info geocoding does, under TIPNR's ids, so the two indexes describe one place, not two). It may also carry the Eras, Lifespans and Events the Timeline shows (the Starter library's Theographic timeline does, naming people and places by TIPNR's ids).

**Place location**:
A modern location a place of the Bible has been identified with, with its coordinates and the Confidence of that identification. A place may have several (Eden has many tentative ones) or none (its site is unknown); the most confident is where the Atlas draws it.
_Avoid_: Coordinates, Geocode, Site (for the record)

**Confidence**:
How sure scholarship is that a place was at a Place location, from the geocoding's score: High (500 and above, as its own notes put it), Medium (100 to 499) or Low (below 100). The Atlas's markers show it (filled, half-filled, hollow) and its lists name it.
_Avoid_: Certainty, Accuracy, Probability

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

**Collection**:
A named set of Resources the user saves in the Library Tab (New Collection…), chosen by hand, by rules (Resource type, language, Resource tag, licence, title contains), or both. A rule-based Collection takes in a Resource as soon as it is downloaded or imported. A Collection can be a Library search's Search scope, the Passage Guide's Commentaries filter, and the Library Tab's filter. Collections are kept in the User database; deleting one never removes its Resources.
_Avoid_: Set, Group, Shelf, Folder (for Resources)

**Resource tag**:
A label of the user's own on a Resource, set in its About Tab (such as "older" or "reformed"), so a Collection's rule can gather the Resources that carry it. Kept in the User database.
_Avoid_: Category, Keyword, Label (as the name)

**Starter library**:
The set of Resources bundled with the installer, available offline from first launch. They are added to the library on the first launch; an installer without them offers to download them under Library → Starter library…, which also lists each one and adds back any that are missing.
_Avoid_: Default library, Base package

**Removed Starter Resource**:
A Starter library Resource the user removed from the library. It is remembered in the User database and never added back silently; Library → Starter library… lists it as Removed, with Add back.
_Avoid_: Deleted resource, Hidden resource

**Release archive**:
The app for one platform as a plain `.tar.gz` or `.zip`, with the Starter library in a `starter` folder beside it, published with each tagged release. Unpacked, it runs as it is and adds the Starter library on the first launch. What users download until there are installers; it is unsigned.
_Avoid_: Installer, Bundle, Package (for the archive)

**Import**:
Adding a Resource to the library from a local file in an open format (USFM or SWORD in v0.1). Every Import produces an Import report.
_Avoid_: Install, Download

**Get Resources**:
The Tab that lists the Bibles, Lexicons, Commentaries and Dictionaries offered by online SWORD repositories and by the Open catalogue, Featured ones first, each one click from download and Import. "Get Bibles" is the same Tab filtered to Bibles. Its licence filter shows only what is Free to share, what is Free for personal use, or All licences.
_Avoid_: Store, Shop, Module manager, Install (for the whole flow)

**Open catalogue**:
The curated list of openly licensed Resources that are not SWORD modules, such as eBible.org's USFM Bibles. Get Resources lists them, and Download fetches each one from its publisher's own URL and imports it with our own importer. Each entry has its licence, Attribution and archived licence evidence. A download is checked first, against a pinned sha256 or against the pinned sha256 of the licence notice inside it, which the evidence quotes.
_Avoid_: Store, Mirror, Extra repository

**Licence class**:
What a Resource's licence lets the user do. There are three. **Free to share**: Public Domain, CC0, CC BY, CC BY-SA or GPL. **Free for personal use**: permission to distribute granted to a repository, non-commercial, or no derivatives. **Unclear**: no licence, or any value not on the two allowlists. A SWORD `DistributionLicense` or SPDX id has a Licence class only if it is, whole, one of the values seen in the live catalogs (or an SPDX id) that the allowlists name; case and spacing aside, nothing else is read, so "Public Domain; no redistribution" or "Not Public Domain" is Unclear. Unclear is never shown as free. An Open catalogue entry declares its Licence class beside its licence evidence.
_Avoid_: Licence type, Free/Non-free, Open (unqualified, for the class)

**Update available**:
What Get Resources shows on a downloaded Resource when its SWORD repository now offers a newer `Version` of the module it was downloaded as; the Library menu counts them. Update (one Resource) or Update all downloads it and imports it as the Resource's next Resource revision. Updates are checked when the catalog is fetched, and nothing is downloaded until the user clicks. When the repositories cannot be reached, or only a catalog saved by an earlier run of the app is known, the Tab says updates could not be checked or when they last were, never that the Resources are up to date. Starter library Resources are updated with the Starter library, not here.
_Avoid_: Upgrade, New version (as the name of the offer)

**SWORD copy**:
The SWORD module files a downloaded Resource was imported from, kept in the app's own SWORD folder. Updating a Resource replaces its SWORD copy; removing the Resource deletes it.
_Avoid_: Module cache, Download folder

**Hebrew & Greek**:
The Get Resources filter, and its Featured group, for Bibles in Hebrew, Aramaic or Greek. Each row shows before download whether the Bible is tagged with Strong's numbers and Morphology, which Word Info, Word Study and Key words need. It filters Bibles by language; it is not a Resource type.
_Avoid_: Originals, Original-language texts (as a type)

**Import report**:
The summary shown after an Import: what was brought in, which markup was dropped, how many Unmapped and Absent verses the Bible has, and any licence information that was missing.
_Avoid_: Import log, Conversion warnings

**Search index**:
A cache built from one Resource so that it can be searched. It is derived data, never shipped as part of the Resource and never user data, and it can be rebuilt at any time.
_Avoid_: Index (unqualified), Search database

**Saved search**:
A search the user kept under a name (Save… beside the query box): its query, the Search Tool's mode (Bible, Library or Notes) and its Search scope. It is listed in the Search Tool's sidebar and offered by the Command box; running one brings all of it back and searches again. Saving under a name already used replaces that one. A Saved search whose Search scope names a Resource or Collection that is gone still runs, over what is left, and says what was left out. Kept in the User database.
_Avoid_: Bookmark (for a search), Search preset, Stored query

**Search history**:
The searches run in the Search Tool, newest first with their mode and Search scope, at most the last 50 different ones, listed in its sidebar under History. Clicking one runs it again; Clear empties it. It is not History (the Tool of Visits), although a search also makes a Visit there, and not the Recent menu, which lists only the query texts of one mode. Kept in the User database.
_Avoid_: History (unqualified, for this), Recent searches

**Search scope**:
What a search runs over: a Passage list and a set of Resources (or Notebooks for notes). A Library search's Resources can be whole Resource types, Collections and single Resources. It is chosen beside the query, not written inside it.
_Avoid_: Filter, Range (for the scope)

**Morphology criteria**:
The grammar one Tagged word must have for a Bible search to find it, named in plain English after `@` in the query (`@verb aorist imperative`): a part of speech, tense, voice, mood, case, number, gender, person, stem or state. They all hold for the same word, and a Strong's number or Lemma typed just before the `@`, with no `AND` between, is that word's too (`strong:G26 @noun genitive`).
_Avoid_: Morph codes, Parsing (for the query), Morph search syntax

**Proximity search**:
A search for two terms close to each other, written between them in the query: `NEAR/5` (at most five words apart, either order), `BEFORE/3` (the first at most three words before the second) and `WITHIN 5 words`, all inside one Verse or paragraph; and `WITHIN 2 verses` (`WITHIN 2 paragraphs` outside Bibles), where both sides are found in a run of at most two neighbouring Verses of one Bible book (or paragraphs of one Dictionary article or Commentary section). A cross-Verse search lists every Verse that takes part, each in its own row, with both sides highlighted. Notes searches refuse `BEFORE` and `WITHIN … verses`.
_Avoid_: Near search, Distance search, Span search

**Word forms**:
The Search Tool's toggle beside the query (off by default, kept in the User database) that makes each English word in Bible and Library searches also find the other words of its stem, the archaic ones included: "love" finds "loved", "loving", "loveth" and "lovest". A "quoted" word, a prefix (`lov*`), a Strong's number and a Lemma stay exact; Greek and Hebrew words are searched by `lemma:` instead.
_Avoid_: Stemming (in the UI), Fuzzy search, Inflections

**Morphology chooser**:
The panel under Bible mode's query box (its **Morphology…** button) with a menu for each kind of grammar; it writes the Morphology criteria chosen into the query, so nobody has to learn the words or the codes. What it writes and the same criteria typed are one query.
_Avoid_: Morph builder, Parsing picker

**Results view**:
How a Bible search's results are shown, switched above them: Verses (the Verse list), Grid (a heat map of matching Verses by Bible book × Bible), Chart (bars of matching Verses by Bible book, or by chapter within one book) or Analysis (a table with one row per matched word: Reference, Bible, the word, its Lemma, Morphology and context, sortable by any column and exported as CSV).
_Avoid_: Mode (the Search Tool's Bible / Library / Notes switch), Report

**Results filter**:
The Bible book or chapter (and, from a Grid cell, the one Bible) that clicking a Grid cell or Chart bar narrows the Verse list to. It shows as a chip above the list until its × clears it, and a new search clears it. The Grid, Chart and Analysis always count every result. It narrows results already found; the Search scope decides what is searched.
_Avoid_: Scope (for this), Drill-down

**Resource revision**:
A corrected release of the same Resource, such as a typo fix in a Bible. It replaces the earlier revision and keeps the Resource's identity, so its Preferred order and Annotations carry over. A downloaded Resource gets one through Update available. A different edition is a different Resource, not a revision.
_Avoid_: Update, Version (for a correction)

**Attribution**:
The license and credit text that a Resource requires to be shown, displayed in that Resource's About view. Text copied with Copy verses, a Notes export or a print page carries the Attribution of each Resource it quotes whose licence asks for credit (anything but public domain or CC0). A quote carries the Attribution of the revision it was taken from. The User database remembers the Attribution of each revision of a Resource seen in the library, so the quotes its kept Annotations hold still carry it after it is removed (or its file goes missing); a Notes export leaves out a quote whose revision's Attribution was never saved, or not saved as of when the quote was taken, and says why.
_Avoid_: Credits, Copyright notice

**Credits**:
The app-level list under Help → Credits: every Starter library Resource, the Resources downloaded from the Open catalogue, the data built into the app, the bundled fonts and the third-party software, each with its licence. A Resource's own Attribution stays in its About view.
_Avoid_: Acknowledgements, About (for this list)

**Change record**:
The published `CHANGES-<id>.md` stating the changes made to a CC BY Starter library source, linked from that Resource's About view.
_Avoid_: Changelog, Diff

### Workspace

**Workspace**:
The live arrangement of Panels and Tabs in the main window and any Floating windows.
_Avoid_: Layout (for the live arrangement), Desktop

**Floating window**:
An OS window of its own holding Panels and Tabs of the Workspace, so they can sit on another monitor. A Tab goes into a new one from its right-click or ⋯ menu (Float in new window) or by being dragged out of its window, and comes back with Move to main window. Closing a Floating window moves its Panels back into the main window. Keys typed in a window act on that window's Tabs. Link sets work across windows, and Layouts and the Last session keep each Floating window with where it was on screen; one whose monitor is gone when it is restored opens on a monitor that is there.
_Avoid_: Popout, Undocked window, Detached Panel

**Panel**:
A dockable, resizable region of the Workspace that holds one or more Tabs.
_Avoid_: Pane, Window, Dock

**Tab**:
One open thing in a Panel: a Resource, a Guide, or a Tool.
_Avoid_: View, Document

**Tool**:
A Tab that is neither a Resource nor a Guide. In v0.1 the Tools are Text Comparison, Word Info, Search, and Notes; Cross-references, Favorites, History, Reading plans, the Atlas and the Timeline came later.
_Avoid_: Utility, Widget

**Word Info**:
The Tool that shows a Tagged word's Lemma, Strong's number, Morphology and Gloss when the user hovers over or clicks the word.
_Avoid_: Info panel, Information tool

**Text Comparison**:
The Tool that shows one Passage in several Bibles as Verse-aligned rows. It can optionally mark the wording that differs from a base Bible.
_Avoid_: Compare, Diff view

**Atlas**:
The Tool that maps the places a Passage names, on a base map built into the app (no tiles, no network). It follows its Link set; a single Verse widens to its chapter, so a Bible at Acts 13:14 maps all of Acts 13. Each place is drawn at its most confident Place location, labelled, and listed beside the map with its Confidence and the Verses that name it; a place no index locates is listed as not on the map. Drag pans, the wheel zooms, Find a place… finds any located place, and clicking a place opens its Factbook.
_Avoid_: Map view, Bible map, Atlas Resource

**Timeline**:
The Tool that shows biblical history on a zoomable horizontal axis: the Eras as a band of named periods, then Lifespans, then Events, each at its dates as the index gives them. It follows its Link set: the Events the Passage tells are emphasised, with the Lifespans of the people they name, and the view centres on the one it tells most of; a Verse no Event tells widens to its chapter. Clicking an Event opens its Passage (its Link set moves there, or the ★ Bible opens at it), clicking a Lifespan opens the person's Factbook, clicking an Era shows all of it. Dates say how sure they are: "c. 2246 BC" where the source marks a date approximate, ranges for what lasts ("1055–1015 BC"), and a day only where the source gives one.
_Avoid_: Chronology (as the Tool's name), Time chart, History view

**Era**:
A named period of the biblical story on the Timeline (the Patriarchs, the United Kingdom, the Exile), dated from the Events of the same chronology so the two agree.
_Avoid_: Period, Age, Epoch

**Lifespan**:
One person's life on the Timeline, from birth to death, as a bar that opens their Factbook.
_Avoid_: Life, Lifetime (as the record's name)

**Event** (on the Timeline):
Something that happened, on a day, in a year or over years (a reign, a journey), with the people and places it names and the Passages that tell it.
_Avoid_: Incident, Happening

**Current location**:
The Key that a Tab is showing right now.
_Avoid_: Position, Cursor

**Contents**:
The outline of a Resource that a Resource Tab shows beside its text when its header's Contents button is on: a Bible's Bible books and chapters; a Commentary's volumes (when it names them), Bible books, chapters and sections, each introduction first; a Lexicon's or Dictionary's entries A–Z, under their first letter, with a filter box. The line of the Tab's Current location is highlighted, and follows the text as it scrolls; clicking a line takes the Tab there, as a jump its Link set follows. It is made from the Resource itself, the first time its Contents are shown.
_Avoid_: TOC, Outline, Index, Navigator

**Link set**:
A lettered group of Tabs (A, B, C…) that share a Current location. When one Tab navigates, every other member whose Resource has that Key scheme follows, translating through Verse maps. A Tab belongs to at most one Link set.
_Avoid_: Sync group, Linked panels

**Parallel resources**:
The other Resources of a Resource Tab's Resource type, in Preferred order. The Tab's header switches it to one of them (◀ ▶, the dropdown, Ctrl+[ / Ctrl+]) at the same Current location, translated through Verse maps; a Commentary without that Passage shows its nearest earlier section. The Tab keeps its Link set, and the switch is a step in its Back/Forward history.
_Avoid_: Parallel Tabs, Resource swap, Synced resources

**Layout**:
A named, saved snapshot of the Workspace, including its Link sets and Floating windows, that the user can restore.
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

**Command box**:
The box at the right of the menu bar (Ctrl+L) that reads what the user types and suggests what to do with it, best first: a Bible reference goes there in the focused Bible, a Strong's number or Lemma (or its transliteration, "agape") opens a Word Study, a Resource or Layout name opens it, a Saved search's name runs it, "pg Rom 8", "eg Jn 3:16", "compare Jn 3:16", "ws agape" and "tg forgiveness" open that Guide or Tool, and anything else searches. An empty box lists its recent entries, kept in the User database.
_Avoid_: Omnibox, Search bar, Command palette (for the box)

**Launcher**:
A toolbar button with an icon and a label (Library, Guides, Tools, Layouts) that drops down what it launches, under itself; Alt+L, Alt+G, Alt+T and Alt+Y open them from the keyboard. The Library launcher searches the library by title, abbreviation, Resource type and language, and with nothing typed lists the Resources opened lately (Recent: those of the newest History Visits and of open Tabs) and the top of each type's Preferred order. The Layouts launcher shows saved and Built-in Layouts as cards with a thumbnail of their Panel tiling.
_Avoid_: Ribbon, Toolbar menu, Pane (for its drop-down)

**Toolbar**:
The bar holding the Launchers. It sits at the Top of the window (in the menu bar) by default, or at the Bottom, Left or Right: its Toolbar position, chosen in View → Toolbar position, by right-clicking it, or by dragging its grip to that edge. At the Left and Right it shows icons only, with the labels in the tooltips. Customize toolbar… shows or hides each Launcher, orders them, and picks icons and labels, icons only or labels only; a Launcher with no room in a small window goes into its » button. With Auto-hide it is drawn over the Workspace only while the pointer is at its window edge or Alt is held. A hidden Launcher still opens from its shortcut and the Command box. All of this is a preference in the User database, never part of a Layout.
_Avoid_: Ribbon, Dock, Menu bar (for the toolbar)

**Keyboard shortcut**:
A key that runs a command, or does something to the reading-pane selection (a Selection key). Each has a default key; Help → Keyboard shortcuts lists them all, grouped by menu and searchable, and there the user clicks one to record a new key, Clears it or Resets it to its default (Reset all restores every default). A key another Keyboard shortcut already has is not saved until the user swaps the two keys or cancels. Esc, Enter and Tab are never Keyboard shortcuts, and a key needs Ctrl or Alt unless it is a function key. Ctrl means Cmd on macOS. The user's own keys are a preference in the User database and apply at once.
_Avoid_: Hotkey, Key map, Accelerator, Key binding (in the UI)

**Settings**:
The Tab (Ctrl+, or View → Settings) where every preference is in one place, under Text and fonts, Interface size, Theme and Page tone, Reading layout defaults, Toolbar, Keyboard shortcuts (the Keyboard shortcuts list itself), Library folders and downloads, Search, and Privacy and data. Its header box searches the Settings by their names, sections, plain words ("zoom", "sepia") and choices. A change there is made by the same command as from the View menu, the toolbar, Home or Search, so it applies at once; each Setting's Reset link puts its default back. Settings is not a separate store: the preferences stay in the User database.
_Avoid_: Preferences (as the Tab's name), Options, Program settings, Settings file

**Selection key**:
A Keyboard shortcut that acts on the selection in a reading pane: Note (Ctrl+Shift+N) and the five Highlight styles (Ctrl+1…5) by default. It is listed under Selection in Help → Keyboard shortcuts.
_Avoid_: Annotation key

**Built-in Layout**:
One of the Layouts that come with the app: "Bible + Commentary", "Word study desk", "Parallel Bibles" and "Reading only". It is made from the library's Preferred order when it is loaded, at the Passage shown then, and cannot be renamed or deleted. One that needs a Resource the library lacks says so instead.
_Avoid_: Preset, Template

**Last session**:
The Workspace as the user left it, restored automatically at the next launch. It is not a Layout.
_Avoid_: Autosave layout

**Home**:
The Tab the app opens on at its first launch, and from the Home button beside Today's reading (Alt+H). Its cards are Today's reading, Continue where you left off (the newest Visit), History and Favorites, the Verse of the day, Quick start (Passage Guide, Word Study, Get Resources, Import) and Your library (how many Resources of each Resource type, with Get Resources for an empty one, and how much of the Starter library is there); each row opens its place with one click. The cards are reordered and hidden like a Guide's sections, and that **Home arrangement** is kept in the User database. With **Open Home at launch** on, Home opens on top of the Last session at every launch. It has no Current location, so it is never a Visit or a Favorite.
_Avoid_: Dashboard, Start page, Welcome screen

**Verse of the day**:
One Passage a day from the app's own short list of well-known Passages, the same all day and the next in the list the next day, shown on Home in the ★ Bible; clicking it reads it there.
_Avoid_: Daily verse, Devotional

**Find bar**:
The bar Ctrl+F opens under a Resource or Text Comparison Tab's header (also the Tab's ⋯ menu, Find…) to find words in that Resource only: every match in the whole Resource (or every row) is marked, the count reads "12 of 48", Enter / Shift+Enter (▼ ▲) move from match to match, scrolling to each, and Esc closes it and clears the marks. It matches as Search does (case, accents, breathings, Hebrew points and final forms ignored), with Match case, Whole word and Ignore diacritics (on by default; then, unless Match case is on, Latin letters also find Greek and Hebrew words by transliteration, "agape" → ἀγάπη). Its marks are an overlay, not Highlights, and are never saved. Search all Resources (Alt+Enter) runs the same words in the Search Tool.
_Avoid_: Search (for this), Find dialog, Quick find

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

**Copy verses**:
The window under Tools → Copy verses… that copies a Passage list from one or more Bibles as plain text, rich text or Markdown, after a preview: the reference before or after the text, verse numbers on or off, paragraphs or one verse per line, quotation marks, and full or abbreviated book names. It also prints them (Print / Save as PDF).
_Avoid_: Copy Bible Verses, Export verses

**Copy preset**:
A named set of Copy verses options the user saved, kept in the User database.
_Avoid_: Template, Style, Copy format

**Bundled font**:
A font shipped inside the app: Gentium Plus for Latin and Greek, Ezra SIL for Hebrew. It is the default Reading font and the fallback when a chosen font is missing or lacks a character.
_Avoid_: Default font, Built-in font

### Reading plans

**Reading plan**:
A schedule for reading Scripture day by day, made from a Reading plan template with a start day, a Bible to read in and the days of the week to read on. Each of its Readings is due on a day and is ticked off when read. It lives in the User database, works offline and needs no account; Tools → Reading plans lists and makes them.
_Avoid_: Devotional, Program, Schedule (as its name)

**Reading plan template**:
What a Reading plan reads and over how many days: Bible in a year (canonical, chronological or M'Cheyne), New Testament in 90 days, Psalms and Proverbs monthly, or a custom template of chosen books over N days. The built-in ones are open data files; with fewer days of the week chosen, the same Passages fit into the same span.
_Avoid_: Preset, Plan type

**Reading**:
The Passages of a Reading plan due on one day and ticked off together. A day may have several (M'Cheyne has four, Psalms and Proverbs two).
_Avoid_: Portion, Assignment, Lesson

**Today's reading**:
The toolbar button showing the Readings due today in the first Reading plan with any, and opening the first unread one in the plan's Bible with one click. Home's Today's reading card lists the Readings due today in each Reading plan, each one click from the plan's Bible.
_Avoid_: Daily verse, Devotion

**Behind**:
A Reading plan is behind by N days when N of its reading days before today still have an unread Reading. **Catch up** moves the unread Readings on, keeping their spacing, so the first is read today and the plan ends later; **Reschedule** keeps the plan's last day and spreads the unread Readings over the reading days from today to it.
_Avoid_: Overdue (as the state's name), Late, Reset

### Guides

**Guide**:
A Tab that takes one Key and shows a report made of ordered Guide sections. Its Guide arrangement is remembered for each kind of Guide.
_Avoid_: Report, Dashboard

**Guide arrangement**:
The order of a Guide's sections, which ones are collapsed, and which are hidden. It is remembered for each kind of Guide, not for each Tab.
_Avoid_: Guide layout, Section settings

**Guide section**:
One block of a Guide that gathers related results (for example commentaries or Cross-references) for the Guide's Key. When it finds nothing it says "None in your library" and offers what would fill it: Get Resources at the matching filter (Get Lexicons…, Get Hebrew & Greek texts…), Add the Starter library… when a Starter library Resource that is not in the Library would, and Import a file…. When the Library already has the Resource type but nothing covers the Key (no Lexicon has an entry for this Strong's number), only Get Resources and Import are offered.
_Avoid_: Panel, Card, Widget

**Passage Guide**:
The Guide whose Key is a Passage. Its sections in v0.1 are Commentaries, Cross-references, Key words, and People & places.
_Avoid_: Exegetical Guide

**Exegetical Guide**:
The Guide whose Key is a Passage, read word by word in its original language: each Tagged word of the first Hebrew (Old Testament) or Greek (New Testament) Bible in Preferred order that has the Passage, reached through that Bible's Verse map. Its sections are Word by word (the word, transliteration, Lemma, Strong's number, Gloss and Morphology in plain English, each word linking to its Word Study), Lexicon summaries (the first Lexicon's short entry for each word) and Grammatical notes (what each Morphology term the words use means). It joins Link sets like the Passage Guide.
_Avoid_: Interlinear, Exegetical report, Word-by-word view

**Word Study**:
The Guide whose Key is a Strong's number. Its sections in v0.1 are Lexicon entries, Translated as, Occurrences, and Forms.
_Avoid_: Bible Word Study, Lexical study

**Factbook**:
The Guide whose Key is one person, people group or place of a People & places index. Its header names it and tells it from others of the same name by its family or region ("John, son of Zebedee"); its sections are Summary (other names, the Name meaning and family), Map (a place at each of its Place locations with their Confidence; for a person, the places they lived in), Key Passages (every Passage that mentions it), Dictionary articles, and Related people and places. Typing a name that several share shows a chooser listing each of them. It is never in a Link set.
_Avoid_: Encyclopedia, Profile, Person page

**Topic Guide**:
The Guide whose Key is a topic, a Headword such as "Forgiveness". Its sections are Topical Bible entries (each Topical index's subtopics with their Passages, whose text shows on hover or inline), Dictionary articles (every other Dictionary's article on it), Key Passages (the Passages the Topical indexes cite most often) and Search (a Library search for it). Typing in its box lists the topics the text names, with their synonyms from the Topical indexes (Pardon: see Sin, forgiveness of). Clicking a Passage moves its Link set there.
_Avoid_: Topical study, Subject guide, Topic page

**Name meaning**:
What a person's or place's name means, from a names Dictionary (Hitchcock's Bible Names in the Starter library), shown in a Factbook's Summary.
_Avoid_: Etymology, Definition

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

**Notes export**:
A Markdown or HTML file made from a Notebook (or all of them) or from the Notes a Notes search found: each Annotation under its Passage, with its quoted text and the Note's text. It can also be printed. It is a copy for reading or sharing, not a backup: the app does not read it back.
_Avoid_: Backup, Report, Notes dump

**User database**:
The single local store of everything the user has made or chosen: Notebooks, Annotations, Layouts, Last session, Preferred order, Guide arrangements, Removed Starter Resources, Favorites, History, Reading plans, the Home arrangement, Saved searches and the Search history, the Command box's recent entries, Copy presets, Collections, Resource tags, the user's own Keyboard shortcuts and preferences. Resources and Search indexes are never part of it.
_Avoid_: Profile, User library, Settings file
