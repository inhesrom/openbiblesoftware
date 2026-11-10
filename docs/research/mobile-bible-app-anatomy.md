# Mobile Bible-app anatomy (#329)

How four leading mobile Bible apps lay out reading and study on phones and tablets, as input to
the "Open Bible" iOS/Android decision: Logos first (the Logos-parity test), then Olive Tree,
Accordance and YouVersion. Every claim below is from the vendor's own help centre, release notes,
blog or store listing, fetched in one session; the newest dated item seen was the YouVersion App
Store release note 11.34.0 (Oct 14, 2026), and Olive Tree articles carry "Updated" stamps from
July to August 2026. No app was installed or run: this is what the vendors document, not what the
apps were seen to do.

## At a glance

| | Logos (Faithlife) | Olive Tree | Accordance | YouVersion |
|---|---|---|---|---|
| Top-level navigation | Bottom bar: Today, Ask, active book / Tabs, Study, More ([Get Started](https://support.logos.com/hc/en-us/articles/360035659011-Get-Started-with-Logos-Mobile)) | Toolbar icons over the reader: Menu, Library, Store, verse header, Plans, Quick Settings, Search, Ribbon, Study Center ([Basics](https://help.olivetree.com/hc/en-us/articles/115005246443-iPhone-iPad-Basics-and-Getting-Started)) | Full-screen "Views" (Main, Search, Library, Notes); Android uses a left Navigation Drawer ([Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm), [Android Dropbox sync](https://accordancefiles2.com/helpfiles/AndroidMobile/Advanced/Content/Topics/02_Install-Update/sycn_dropbox.htm)) | Bottom tabs incl. Bible, Discover, You ([Bible Search](https://help.youversion.com/l/en/article/wakrw3pir7-bible-search-ios), [Discover](https://help.youversion.com/l/en/article/yl7ftkwa8r-discover-ios), [Reader Settings](https://help.youversion.com/l/en/article/j2nmx67p54-settings)) |
| Reading surface | One book per tab; tabs/layouts **off by default** since Logos 54 ([Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts)) | Main window + one split window (Study Center) ([Study Center](https://help.olivetree.com/hc/en-us/articles/115005333463-iPhone-iPad-Study-Center)) | Main pane + optional additional pane ([Tool View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/tool_view.htm)) | One version; two-version Parallel Mode only in landscape ([Compare/Parallel](https://help.youversion.com/l/en/article/nxlpf1hcgh-compare-bible-version-ios)) |
| Word tap | Press-and-hold word → Insights Panel (bottom sheet; right-hand pane on tablet) ([Insights Panel](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel)) | Tap word in Strong's/parsed text → pop-up → Lookup ([How To: BDAG](https://help.olivetree.com/hc/en-us/articles/18556715619981-How-To-BDAG)) | Long tap (or opt-in single tap) → Instant Details pop-up ([Instant Details](https://accordancefiles2.com/helpfiles/AndroidMobile/Advanced/Content/Topics/04_Work/instant_details.htm), [New in v3](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/01_Welcome-About/newV3.htm)) | Tap selects whole verse → actions menu; no word lookup documented ([Highlights](https://help.youversion.com/l/en/article/ie1gz0nsr7-highlights-ios)) |
| Passage study | Passage Guide, Exegetical Guide, Bible Word Study (online only) ([Using Guides](https://support.logos.com/hc/en-us/articles/360017893592-Using-Guides)) | Resource Guide follows the reader in the split window ([Resource Guide](https://help.olivetree.com/hc/en-us/articles/45420760280973-iPhone-iPad-Resource-Guide)) | No guide; Amplify + parallel commentary pane | Study-notes card in the verse actions sheet ([Bible Study Notes](https://help.youversion.com/l/en/article/ozcokhdkwc-bible-study-notes-ios)) |
| Offline | Streams by default; per-book download; Guides/library search need internet ([Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison)) | Downloaded books work offline ([Basics](https://help.olivetree.com/hc/en-us/articles/115005246443-iPhone-iPad-Basics-and-Getting-Started)) | Fully offline after setup ([Details of Accordance Mobile](https://www.accordancebible.com/details-of-the-mobile-app/)) | Selected versions downloadable; search needs internet ([Offline Bible](https://help.youversion.com/l/en/article/e3fjtnsoil-download-offline-bible-ios), [Lite FAQ](https://help.youversion.com/l/en/article/80mxibuu5i-faq)) |
| Annotation sync | Faithlife account | Olive Tree account ("Study Sync") ([Study Sync](https://help.olivetree.com/hc/en-us/articles/115005333363-iPhone-iPad-Study-Sync)) | **User's Dropbox** or Wi-Fi sync to Mac ([Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm)) | YouVersion account ([App Store listing](https://apps.apple.com/us/app/bible/id282935706)) |

## Logos (the parity target)

1. **Navigation model: bottom bar plus a "Study" hub.** The bottom bar holds Today (plans,
   Quickstart layouts, featured content), Ask (Study Assistant), the active book / tab count,
   Study ("access to all of the areas and features") and More (settings, Top Bible). Tablets
   add Store and Layouts icons. The Study tab is "home base": a ribbon of Top Bible and recent
   books, then Pinned, Read (Library, Audiobooks, Reading Plans), Find Answers (search, Study
   Assistant), Study (Guides, Factbook, Text Comparison), Notes & Documents, Advanced Tools.
   [Get Started with Logos Mobile](https://support.logos.com/hc/en-us/articles/360035659011-Get-Started-with-Logos-Mobile)
2. **Reading-first by default.** Since Logos 54 a fresh install keeps one tab open ("allowing you
   to focus on a particular book or tool"); multiple tabs and layouts are a toggle in More.
   [Set Up Your Workspace with Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts)
3. **Passage navigation.** The top bar offers: tap title for parallel books, left/right arrows to
   move position, tap the location bar to go anywhere, search icon for in-book search, audio,
   overflow menu (view settings). [Get Started](https://support.logos.com/hc/en-us/articles/360035659011-Get-Started-with-Logos-Mobile).
   A mobile-only Reference Scanner opens references read by the camera (Logos Plus and up).
   [Reference Scanner](https://support.logos.com/hc/en-us/articles/360026651951-Reference-Scanner)
4. **Split screen and linking.** In the Tabs view, a split icon adds a second panel below; panels
   join link sets A/B/C and can be "Follow only". Phones hold two panes per screen, tablets four;
   tablets offer eight layout templates. Drag the divider to the edge to remove a split.
   [Mobile – Linking Tabs](https://support.logos.com/hc/en-us/articles/360025293271-Mobile-Linking-Tabs),
   [Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts).
   Saved layouts sync phone↔phone and tablet↔tablet, **not with desktop**. [Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts).
   Desktop's in-panel "Multiple Books View" is replaced by split screens on mobile.
   [Viewing Multiple Books Together](https://support.logos.com/hc/en-us/articles/360016304832-Viewing-Multiple-Books-Together)
5. **Word lookup on tap = Insights Panel (a resizable bottom sheet).** Press-and-hold a word opens
   it on the word; double-tap opens it on the verse. A slider sets height, a toggle switches
   word/verse focus, arrows pan to the next word/verse. Sections: recent highlight styles;
   Guides and Tools (Copy, Share, Note, Search, Bible Word Study, Passage Guide, Study Assistant,
   Text Comparison); then expandable cards: Word Info (lemma, root, morphology, Strong's, top
   lexicon entry), Notes & Highlights, Factbook, Related Books (top study Bible + commentary),
   Related Passages (cross-reference and harmony books, NT use of OT), Sermons & Bible Studies.
   **On a tablet the panel opens to the right of the text as a separate pane.** In non-Bible
   books a Lookup Card shows the top dictionary/encyclopedia entry. Premium tier required on
   mobile. [Using the Insights Panel](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel)
6. **Cross-references** surface inside the Insights Panel (verse focus) and the Passage Guide's
   Cross References section, which shows the text of the related passages.
   [Insights Panel](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel),
   [Passage Guide](https://support.logos.com/hc/en-us/articles/360016462872-Study-a-Passage-Using-the-Passage-Guide)
7. **Guides on mobile: three, fixed, cut down, online.** Mobile has Bible Word Study, Passage Guide
   and Exegetical Guide (Study tab → Study). [Using Guides](https://support.logos.com/hc/en-us/articles/360017893592-Using-Guides).
   The mobile Passage Guide has seven fixed sections (Commentaries, Cross References, Parallel
   Passages, Literary Typing, Media, Topics, Interesting Words) vs. desktop's add/remove/reorder
   section stack. [Passage Guide](https://support.logos.com/hc/en-us/articles/360016462872-Study-a-Passage-Using-the-Passage-Guide).
   Mobile Word Study has three sections: Definition, Translation (ring chart over the Top Bible),
   Example Uses. [Bible Word Study](https://support.logos.com/hc/en-us/articles/360016688811-Studying-Words-Using-the-Bible-Word-Study-Guide).
   "Guides only work when online", need sign-in, and show "a small subset" of desktop results.
   [Logos Mobile FAQ](https://support.logos.com/hc/en-us/articles/360020441372-Logos-Mobile-FAQ)
8. **Notes and highlights.** Highlight styles sit at the top of the Insights Panel (up to 20
   recent); dragging text without holding applies the last style. [Insights Panel](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel).
   Mobile highlighting and notes are "partial": no Corresponding Highlights/Notes, no highlight
   Labels. [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison).
   Also unsupported: note tags, turning off individual note documents, writing notes with Apple
   Pencil. [FAQ](https://support.logos.com/hc/en-us/articles/360020441372-Logos-Mobile-FAQ)
9. **Search.** In-book Inline Search (smart, filter, All Search) from the top-bar icon or the
   Insights Panel; full Bible search with version and range pickers.
   [Inline Search](https://support.logos.com/hc/en-us/articles/360016524471),
   [How to Search Your Bible](https://support.logos.com/hc/en-us/articles/360017808111-How-to-Search-Your-Bible-Using-Logos).
   Mobile supports "a small subset" of desktop search; no wildcards; library-wide search needs
   internet. [FAQ](https://support.logos.com/hc/en-us/articles/360020441372-Logos-Mobile-FAQ),
   [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison)
10. **Library and downloads.** The app streams resources; nothing is stored by default. Users tap a
    per-book download icon, filter by Downloaded, or multi-select and download. Downloads are
    per device; desktop's Library info pane can push books to a named mobile device.
    [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison),
    [FAQ](https://support.logos.com/hc/en-us/articles/360020441372-Logos-Mobile-FAQ),
    [Find Your Books in the Library](https://support.logos.com/hc/en-us/articles/360016752591-Find-Your-Books-in-the-Library).
    Mobile can use but not create Collections, and cannot change resource priority.
    [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison)
11. **Left out on mobile (vendor's own list).** Bible Sense Lexicon, Bible Browser (partial),
    Bible Outline Browser, Syntax/Morph Query, Sentence Diagrams, Word Lists, Concordance, Power
    Lookup, Cited By, Copy Bible Verses, Passage Analysis, Explorer, Guide Editor, Sermon Starter
    and Theology guides, Workflows (partial), Personal Books, Print/Export, reverse-interlinear
    ribbon, per-panel fonts, interactive media, "extended offline usage". Atlas opens the web app;
    Canvas, Advanced Timeline and Draw on Screen are iPad-only; Sermon tools are tablet-only. The
    FAQ sums up: mobile apps "only offer a few of the hundreds of features available in Logos
    Desktop". [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison),
    [FAQ](https://support.logos.com/hc/en-us/articles/360020441372-Logos-Mobile-FAQ).
    Two lists disagree in places (the FAQ calls Sermon Starter Guide unsupported; the comparison
    marks it partial), so the FAQ list may be older.

## Olive Tree Bible Study

1. **Navigation model: icon toolbar over a single reader, plus a split window.** Icons: Main Menu
   (messages, history, annotations, sync, help), Library, Store, Verse Header, Reading Plans,
   Quick Settings, Search, Book Ribbon, Study Center. Tap the screen to show the toolbar.
   [Basics](https://help.olivetree.com/hc/en-us/articles/115005246443-iPhone-iPad-Basics-and-Getting-Started)
2. **Passage navigation: tap the verse header → grid.** Book grid → chapter → verse; options for
   compact grid, list (with alphabetical order) and genre shading. Works in the main window and
   the Parallel window. [Navigation](https://help.olivetree.com/hc/en-us/articles/115005324826-iPhone-iPad-Navigation),
   [Using Bibles](https://help.olivetree.com/hc/en-us/articles/4404188579725-Using-Bibles)
3. **Split window = "Study Center" with four tabs:** Resource Guide, Parallel, Notes, Lookup. In
   portrait it is a bottom pane resized by a drag handle (double-tap for full screen); in
   landscape a side pane with a centre divider; dragging below minimum closes it. Window linking
   has three states: Do Not Link, Unlink on Scroll, Never Unlink.
   [Study Center](https://help.olivetree.com/hc/en-us/articles/115005333463-iPhone-iPad-Study-Center).
   Same layout on iPhone and iPad ("steps will be the same").
   [iPhone/iPad section](https://help.olivetree.com/hc/en-us/sections/115001517243-iPhone-iPad)
4. **Resource Guide is Olive Tree's "Passage Guide".** It follows the main window and lists
   material from **downloaded** books: Related Verses, Commentaries, Bibles, maps and more, with
   counts per book; sections and books per section are configurable; a lock icon pins or frees
   it. [Resource Guide](https://help.olivetree.com/hc/en-us/articles/45420760280973-iPhone-iPad-Resource-Guide),
   [Study Center](https://help.olivetree.com/hc/en-us/articles/115005333463-iPhone-iPad-Study-Center)
5. **Cross-references**: Related Verses in the Resource Guide, or a cross-reference book (e.g.
   TSK) opened in Parallel; tapping a reference shows the verse in a pop-up, with a diagonal arrow
   to open it in the main or split window. [Using Cross References](https://help.olivetree.com/hc/en-us/articles/4404604771597-Using-Cross-References),
   [Resource Guide](https://help.olivetree.com/hc/en-us/articles/45420760280973-iPhone-iPad-Resource-Guide)
6. **Word lookup on tap.** In a Strong's-tagged or parsed text, tapping a word opens a pop-up with
   "Lookup [word]" and Search; lexicon articles open in a pop-up that can be promoted to the main
   window or Parallel. [How To: BDAG](https://help.olivetree.com/hc/en-us/articles/18556715619981-How-To-BDAG),
   [Interlinear Bibles](https://help.olivetree.com/hc/en-us/articles/360002934972-Using-Interlinear-Bibles)
7. **Notes and highlights.** Tap a verse number for a verse-based annotation (shown in every
   downloaded translation) or select text for a word-based one (tied to that book). Highlights
   have label, tags, category and custom highlighters (colour or underline); managed from the
   Menu. [Highlights](https://help.olivetree.com/hc/en-us/articles/115005332423-iPhone-iPad-Highlights-and-Highlighters),
   [Annotation Overview](https://help.olivetree.com/hc/en-us/articles/115005325003-iPhone-iPad-Annotation-Overview).
   Synced through the Olive Tree account. [Study Sync](https://help.olivetree.com/hc/en-us/articles/115005333363-iPhone-iPad-Study-Sync)
8. **Search** works on downloaded books: Smart Results (paid service, daily limit) above Exact
   Results; filter by book, saved ranges, history, AND/OR commands, plain-text vs morphology.
   [Search](https://help.olivetree.com/hc/en-us/articles/115005333543-iPhone-iPad-Search-Including-new-Smart-Results-feature),
   [Advanced Search Syntax](https://help.olivetree.com/hc/en-us/articles/203614325-Olive-Tree-Advanced-Search-Syntax)
9. **Library**: Cloud tab (owned, dimmed with arrow until downloaded; tap to download), Device tab
   (downloaded), Recommended; filter, sort, search (library first, then store); "Automatically
   Download Books" setting; per-volume download of commentary sets; audio streams unless
   downloaded. [Library](https://help.olivetree.com/hc/en-us/articles/115005324726-iPhone-iPad-Library),
   [Downloading Books](https://help.olivetree.com/hc/en-us/articles/115005324543-iPhone-iPad-Downloading-Books)
10. **Desktop vs mobile.** Olive Tree's Mac app uses the same Study Center and Resource Guide
    model ([Mac Study Center](https://help.olivetree.com/hc/en-us/articles/29762580603021-Mac-Study-Center));
    no first-party list of mobile-only omissions was found.

## Accordance

1. **Navigation model: full-screen Views.** Main View (reading), Search View, Library View, Notes;
   Library has a bottom bar (Easy Install, Sync, Settings, Store, Help). On Android a Navigation
   Drawer (swipe from left) holds Syncing and other areas.
   [Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm),
   [Search View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/search_view.htm),
   [Sync with Dropbox (Android)](https://accordancefiles2.com/helpfiles/AndroidMobile/Advanced/Content/Topics/02_Install-Update/sycn_dropbox.htm)
2. **Passage navigation: Verse Chooser** (grid by default, or list): book → chapter → verse; tools
   use a searchable TOC list; bottom-bar arrows step chapters (or hits after a search).
   [Navigation within a Resource](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/04_Work/navigation_in_resources.htm),
   [Tool View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/tool_view.htm)
3. **Parallel: one additional pane.** "Additional Pane" button opens a second pane (bottom in
   portrait); iPad can choose horizontal/vertical split; panes link by default and can be
   unlinked; a Text in pane 1 can show any Tool or User Notes in pane 2.
   [Tool View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/tool_view.htm),
   [New in v3](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/01_Welcome-About/newV3.htm),
   [Unlinking Additional Panes](https://www.accordancebible.com/Unlinking-Additional-Panes-Accordance-Mobile/).
   Apart from screen size, the iOS app "is identical on all iOS devices".
   [Details of Accordance Mobile](https://www.accordancebible.com/details-of-the-mobile-app/)
4. **Word lookup: Instant Details pop-up** on long tap (single tap is an opt-in setting since 3.3;
   a simple tap on a footnote or cross-reference shows it). Offers Search or Amplify for the word
   or key number (lemma/root/inflected for tagged texts); drag the handle up for full screen,
   swipe down to return to overlay. [Instant Details](https://accordancefiles2.com/helpfiles/AndroidMobile/Advanced/Content/Topics/04_Work/instant_details.htm),
   [New in v3](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/01_Welcome-About/newV3.htm)
5. **Cross-references** ship as a module (margin notes and cross-references in the free bundle)
   and show in Instant Details when tapped. [App Store listing](https://apps.apple.com/us/app/accordance-bible-software/id411970514)
6. **No Passage Guide / Word Study** is documented for mobile; study happens through Amplify,
   Instant Details and the parallel pane (no guide pages found in the Mobile help pages checked).
7. **Notes and highlights.** Long-press a verse reference → User Note into a chosen notes file;
   notes show as icons, in a pop-up, in a linked pane, or in Notes View. User Tools and Papers
   from desktop are **read-only** on iOS. [Notes View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/notes_view.htm),
   [Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm)
8. **Sync through the user's Dropbox (precedent for a user-chosen cloud folder).** Notes and
   highlights sync between devices and Mac/Windows via a Dropbox "Apps" folder, or by Wi-Fi to a
   Mac. [Sync with Dropbox](https://accordancefiles2.com/helpfiles/AndroidMobile/Advanced/Content/Topics/02_Install-Update/sycn_dropbox.htm),
   [Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm).
   Help for desktop notes that conflicting edits prompt a choice of version
   ([Win13 Sync to Dropbox](https://www.accordancefiles1.com/helpfiles/Win13/content/topics/02_install_etc/sync_dropbox.htm), not fetched directly; from a search summary).
9. **Search** is a strength: commands, ranges, grammatical/key-number search, all offline.
   [Search Commands](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/04_Work/search_commands.htm),
   [Details](https://www.accordancebible.com/details-of-the-mobile-app/)
10. **Library and downloads.** Ships with a bundle (ESV with Strong's, WEB, dictionaries, cross-
    references, outlines); free account adds a Starter collection; Easy Install lists purchases
    and updates, multi-select download; modules can be deleted to free space.
    [App Store listing](https://apps.apple.com/us/app/accordance-bible-software/id411970514),
    [Easy Install](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/02_Install-Update/easy_install_updates.htm),
    [Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm).
    Android APK (2.2.3) is also offered directly from Accordance's own server, updated manually.
    [Accordance for Android](https://www.accordancebible.com/accordance-for-android/)
11. **Left out on mobile:** Atlas, Timeline and parallel databases are "NOT supported" on iOS;
    User Tools and Papers cannot be edited. [Easy Install](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/02_Install-Update/easy_install_updates.htm),
    [Library View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/library_view.htm).
    The Android product page still says "Future updates will add … User Notes", which conflicts
    with the Android Dropbox help (notes and highlights); the product page looks stale.
    [Accordance for Android](https://www.accordancebible.com/accordance-for-android/)

## YouVersion (the reading baseline)

1. **Navigation model: bottom tabs** (Bible, Discover, You, plus Home/Plans per older docs);
   settings live under You → Settings.
   [Reader Settings](https://help.youversion.com/l/en/article/j2nmx67p54-settings),
   [Discover](https://help.youversion.com/l/en/article/yl7ftkwa8r-discover-ios)
2. **Passage navigation:** tap the "John 1" bar → book list (traditional or alphabetical) →
   chapter; the version abbreviation beside it opens the version picker; Recents in the overflow
   menu. [Change Bible Version](https://help.youversion.com/l/en/article/050z6oe74d-change-bible-version-or-language-ios),
   [Reading History](https://help.youversion.com/l/en/article/phc6srbyii-bible-reading-history).
   Bible search also accepts references ("1 Peter 4:1").
   [Bible Search](https://help.youversion.com/l/en/article/wakrw3pir7-bible-search-ios)
3. **Tap = verse selection + bottom actions sheet.** Tapping verses underlines them and shows an
   actions menu with highlight colours; swipe it for Compare; swipe it up for a Biblica Study
   Bible card and "See Also" book/chapter notes.
   [Highlights](https://help.youversion.com/l/en/article/ie1gz0nsr7-highlights-ios),
   [Compare/Parallel](https://help.youversion.com/l/en/article/nxlpf1hcgh-compare-bible-version-ios),
   [Bible Study Notes](https://help.youversion.com/l/en/article/ozcokhdkwc-bible-study-notes-ios)
4. **Parallel: two versions, landscape only;** verse actions only on the right side. "Compare"
   lists one verse across chosen versions. [Compare/Parallel](https://help.youversion.com/l/en/article/nxlpf1hcgh-compare-bible-version-ios)
5. **Cross-references and footnotes** are a reader setting; tappable cross-references that keep
   your place were announced April 2026. [Reader Settings](https://help.youversion.com/l/en/article/j2nmx67p54-settings),
   [Tappable Cross-References](https://blog.youversion.com/2026/04/tappable-cross-references/)
6. **No word lookup, guides or lexicons** are documented; search is keyword-based, one version at a
   time, relevance-ordered, and needs internet.
   [Bible Search](https://help.youversion.com/l/en/article/wakrw3pir7-bible-search-ios),
   [Lite FAQ](https://help.youversion.com/l/en/article/80mxibuu5i-faq)
7. **Library = version list.** Pick a version → Add to download (signed in); one default version
   per language ships with the app and cannot be removed; audio not downloadable.
   [Offline Bible](https://help.youversion.com/l/en/article/e3fjtnsoil-download-offline-bible-ios)
8. **Notes, highlights, bookmarks** sync via a free account; notes private by default, shareable
   or public. [App Store listing](https://apps.apple.com/us/app/bible/id282935706),
   [Notes (Android)](https://help.youversion.com/l/en/article/wb7u0jdlgn-notes-android)
9. **Desktop:** none; the counterpart is the bible.com web app, which has no offline Bibles.
   [Bible App Web](https://help.youversion.com/l/en/article/qi1gf7u75f-bible-com-bible)

## Tablet layouts compared

| App | Phone | Tablet |
|---|---|---|
| Logos | 2 panes via split; Insights Panel as bottom sheet | Up to 4 panes, 8 templates; Insights Panel pinned right; Canvas/Timeline/Sermon tools ([Linking Tabs](https://support.logos.com/hc/en-us/articles/360025293271-Mobile-Linking-Tabs), [Insights](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel)) |
| Olive Tree | Study Center as bottom pane (portrait) | Same model; side-by-side in landscape ([Study Center](https://help.olivetree.com/hc/en-us/articles/115005333463-iPhone-iPad-Study-Center)) |
| Accordance | Main + additional pane | Same, plus horizontal/vertical choice, multi-column paging ([Tool View](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/03_Views/tool_view.htm), [New in v3](https://accordancefiles2.com/helpfiles/Mobile2/Content/Topics/01_Welcome-About/newV3.htm)) |
| YouVersion | Single version; parallel in landscape | Same (no tablet-specific layout documented) ([Compare/Parallel](https://help.youversion.com/l/en/article/nxlpf1hcgh-compare-bible-version-ios)) |

## Implications for Open Bible mobile

- **One pane + sheet is the industry shape.** Logos (Insights Panel), Olive Tree (Study Center),
  Accordance (Instant Details + additional pane) and YouVersion (actions sheet) all keep one
  reader and pull study into a resizable bottom sheet or second pane; Logos and Olive Tree turn
  that sheet into a side pane on tablets. This matches the planned phone/tablet split.
- **Logos parity is about the tap sheet, not the desktop workspace.** Logos itself ships one tab
  by default, three fixed-section Guides, partial notes/highlights and a long "unsupported" list.
  A word/verse sheet with Word Info, cross-references, notes/highlights and a link into a
  Passage Guide and Word Study meets the Logos mobile bar.
- **Offline is a differentiator.** Logos Guides and library search need internet; YouVersion
  search needs internet. Our engine runs Guides and search locally over `.obr` files, as
  Accordance and Olive Tree do with downloaded books.
- **Cloud-folder sync has precedent.** Accordance syncs notes and highlights through the user's
  own Dropbox; everyone else uses a vendor account. Our design sits with Accordance.
- **Sideloaded APK has precedent.** Accordance hands out a manually updated Android APK from its
  own server alongside the store.
- **Bundled Bible + in-app library** matches Accordance (ESV/WEB bundle + Easy Install), YouVersion
  (one undeletable default version) and Olive Tree (Cloud vs Device tabs).
- **Passage navigation** is a book → chapter → verse grid everywhere (Olive Tree, Accordance,
  YouVersion), with Logos adding free-text reference entry; we should offer both.

## Open questions for the decision ticket

1. Tap gesture: single tap on a word (Olive Tree; Accordance opt-in) or press-and-hold (Logos,
   Accordance default)? Single tap clashes with verse selection (YouVersion) and toolbar toggling.
2. Word sheet vs verse sheet: one sheet with a word/verse toggle (Logos) or two entry points?
3. Do Guides open in the sheet, as a full-screen page, or in the second pane on tablets?
4. Tablet: fixed two-pane (Olive Tree, Accordance) or Logos-style templates up to four panes?
5. Do we mirror Olive Tree's verse-based vs word-based annotations (verse annotations visible in
   every Bible)? How does that map to the User database's anchors?
6. Do desktop layouts sync to mobile? Logos says no; a cloud-folder design could still choose.
7. Which desktop features are out of scope for v1 (mirror the Logos unsupported list: diagrams,
   word lists, query builders, print/export)?

## Gaps

- No app was installed; layouts are as documented, and screenshots in help pages were not read.
- Olive Tree publishes no list of desktop features missing on mobile.
- YouVersion's current exact bottom-tab set (Home/Plans vs Discover/You) was not confirmed from
  one first-party page.
- Accordance Android vs iOS feature gap (User Notes) is ambiguous between product page and help.
- Vendor videos were not reviewed.
