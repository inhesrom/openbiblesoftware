# Logos lay-study feature anatomy (issue #5)

**Question:** How do Logos Bible Software's core lay-study features behave (panels, layouts, link sets, guides, notes, search, reading plans, info/hover, navigation), and which does Logos itself treat as entry-level?
**Date:** 2025 research pass (clean-room: behaviour/concepts only, from public docs; no assets, wording, or trade dress copied).

> **Verification caveat (read first).** support.logos.com and community.logos.com returned **HTTP 403** to direct fetch from this environment. Claims about Help Center articles below were gathered from search-engine indexing of those exact URLs (titles + indexed text), not from a full direct read. Each claim links to the owning article. Treat fine-grained details (exact section names, menu labels) as **"indexed, not directly read"** and spot-check in a browser before using them as specs. logos.com marketing pages were fetched directly.

## TL;DR

- Logos's workspace = **panels holding tabs**, drag-to-dock, tile arrangements, float/undock to other monitors, and **saved named layouts** (snapshots of open panels + link sets). [Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts)
- **Link sets** (letters A, B, C…; "None"; mobile adds "follow only") make panels scroll together; sync only works when resources share a **reference index/datatype** (Bible verse ↔ commentary verse works; page-numbered book doesn't). Versification differences handled by **verse maps** behind the scenes. [Link](https://support.logos.com/hc/en-us/articles/360016827711-How-to-Link-Resources-to-Scroll-Together), [Read together](https://support.logos.com/hc/en-us/articles/360036146912-3-How-to-Read-Your-Bible-and-other-Resources-Together)
- **Library prioritization** (drag-ordered list) drives which book opens for lookups/guides; top 5 Bibles = **"Top Bibles"** used by search/compare. **Collections** = rule-based or hand-picked book groups; can be shown as **parallel-resource sets** (arrow-cycle between books with same location). [Prioritize](https://support.logos.com/hc/en-us/articles/360019683652-Prioritize-Books), [Top Bibles](https://support.logos.com/hc/en-us/articles/360029571712-What-are-Top-Bibles-and-how-do-I-change-them), [Collections](https://support.logos.com/hc/en-us/articles/360016299092-Create-and-Use-Book-Collections)
- **Guides** = report panels: one input key (passage, word, topic) → ordered, user-customizable **sections**, each section also openable standalone; guide remembers last arrangement. Passage Guide, Exegetical Guide, Bible Word Study, Topic Guide. [Using Guides](https://support.logos.com/hc/en-us/articles/360017893592-Using-Guides)
- **Factbook** = entity-centric hub (person/place/event/topic/passage) aggregating summary + key article + dictionary hits + passages. [Factbook](https://support.logos.com/hc/en-us/articles/360016146691-What-can-I-do-with-the-Factbook)
- **Notes/highlights**: notes and highlights live in **notebooks**; highlights come from **palettes of styles**; a note can have **multiple anchors** (resource location or Bible reference); Bible highlights can show **across versions** ("corresponding text"). [Notes](https://support.logos.com/hc/en-us/articles/360017978372-Record-Your-Insights-Using-Notes), [Anchors](https://support.logos.com/hc/en-us/articles/360031314832-How-Do-I-Add-and-Manage-Note-Anchors)
- **Search kinds**: Books (formerly "Basic"), Bible, Morph, Syntax; Bible search result views = Passages, Aligned, Grid, Analysis. [Search](https://support.logos.com/hc/en-us/articles/360039621591-How-Do-I-Search-in-Logos), [Bible search](https://support.logos.com/hc/en-us/articles/360017808111-How-to-Search-Your-Bible-Using-Logos)
- **Free tier** includes Passage Guide, Bible Word Study (reduced), Factbook, Text Comparison, search, notes, highlights, reading plans, link sets — so Logos itself treats these as **entry-level**; paid tiers add books, AI, Insights (desktop), original-language depth. Free tier **requires a Logos account** (contrast: our offline/no-account decision). [Free](https://support.logos.com/hc/en-us/articles/360023029051-Get-started-with-Logos-for-free), [Pricing](https://www.logos.com/logos-explore-pricing)
- Most "smart" features are **data-bound**, not UI-bound: their value comes from Faithlife's proprietary datasets (lemma tagging, senses, Bible Knowledgebase people/places, cross-ref sets). Our feasible subset is gated by public-domain data availability.

## 1. Workspace: panels, tabs, tiling, layouts

| Concept | Behaviour (per docs) | Source |
|---|---|---|
| Panel / tab | Books, tools, guides open as tabs in panels; drag tab to move/dock; drag edges to resize | [Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts) |
| Tile arrangements | Layouts menu offers preset tile arrangements that rearrange *already-open* panels; don't open/close resources or save | same |
| Saved layouts | Snapshot current workspace; "Save as named layout"; "Update Active" to persist later changes; link sets preserved in layout | same; [Link](https://support.logos.com/hc/en-us/articles/360016827711-How-to-Link-Resources-to-Scroll-Together) |
| Float / dock | Panel More-Actions menu: "Float this panel" / "Dock this panel"; floats can go to another monitor | [More Actions](https://support.logos.com/hc/en-us/articles/360016752571-More-Actions-Menu), [Two monitors](https://support.logos.com/hc/en-us/articles/29402772752397-How-do-I-open-Logos-on-two-monitors) |
| Platform variance | Mobile layouts separate (don't sync with desktop); web has limited layout customization | [Layouts](https://support.logos.com/hc/en-us/articles/360016599631-Set-Up-Your-Workspace-with-Layouts) |

Per-panel chrome: each resource has a **reference/navigation box** (focus with Ctrl/Cmd+G), and prev/next verse/chapter/article via Alt+Up/Down (Win) or Shift+Cmd+Up/Down (Mac). [Shortcuts](https://support.logos.com/hc/en-us/articles/360016370932-Keyboard-Shortcuts). Typing a reference in either of two linked panels navigates both. [Free start](https://support.logos.com/hc/en-us/articles/360023029051-Get-started-with-Logos-for-free)

## 2. Link sets and reference matching

- Set via each resource's Home toolbar → Link Set → same letter (A…); tools/guides without Home toolbar use More Actions (⋮). "None" unlinks. Mobile offers **Follow only** (one-way). [Link](https://support.logos.com/hc/en-us/articles/360016827711-How-to-Link-Resources-to-Scroll-Together)
- **Matching rule:** resources must share a compatible **reference index**. Bible + commentary both indexed by verse → sync. Book indexed only by page → generally can't. [Read together](https://support.logos.com/hc/en-us/articles/360036146912-3-How-to-Read-Your-Bible-and-other-Resources-Together)
- **Datatypes:** Logos internally tags milestones with datatypes (e.g., Bible references in a specific versification). Community wiki lists Bible datatypes and verse maps (Logos 8/9-era, maybe not exhaustive). [Bible Datatypes wiki](https://community.logos.com/wiki/logos-user-wiki/personal-books/list-of-datatypes/bible-datatypes/) — *community, not first-party; indexed only*.
- **Verse maps** reconcile versifications (e.g., Ps 3:1 English = Ps 3:2 BHS); user can force a map in a reference (e.g., `Psalm 118 LXX-R`). [Community tip](https://community.logos.com/forums/topic/181563-verbum-tip-4y-aside-datatype-bible-reference-and-milestone/) — *community source; unverified against official docs*.
- Implied model: link set = broadcast "current location" event in a canonical reference space; each subscriber translates via its index (+ verse map) or ignores if no compatible index.

## 3. Library, prioritization, collections, parallel resources

- **Prioritize:** Library → right-click → "Prioritize this book"; drag order in priority pane; commentaries can prioritize volume or whole series. Priority decides which resource opens for lookups/guides. [Library](https://support.logos.com/hc/en-us/articles/360016752591-Find-Your-Books-in-the-Library), [Prioritize](https://support.logos.com/hc/en-us/articles/360019683652-Prioritize-Books)
- **Top Bibles** = five highest-prioritized Bibles; feed Bible search and compare defaults. [Top Bibles](https://support.logos.com/hc/en-us/articles/360029571712-What-are-Top-Bibles-and-how-do-I-change-them)
- **Collections:** named set defined by rule (e.g., `type:commentary`) plus/minus hand-picked books; usable as search scope and in guides; option "Show in parallel books". [Collections](https://support.logos.com/hc/en-us/articles/360016299092-Create-and-Use-Book-Collections)
- **Parallel books:** from cover icon beside nav box, pick a set; left/right arrows cycle books at same passage/entry; only books containing that location appear; only downloaded books. [Parallel](https://support.logos.com/hc/en-us/articles/360018068751-Working-with-Parallel-Books)

## 4. Guides

General: guide takes one or more **key** inputs (passage range for Passage Guide, word for BWS); sections added via Add, removed via X, reordered by drag; next launch reuses last arrangement; any section can open as standalone guide. [Using Guides](https://support.logos.com/hc/en-us/articles/360017893592-Using-Guides), [Cross Refs section](https://support.logos.com/hc/en-us/articles/360018252931-Cross-References-Guide-Section)

| Guide | Input | Sections (indexed from docs; vary by tier/library) | Source |
|---|---|---|---|
| Passage Guide | Verse/range | Commentaries, Journals, Cross References (default), Parallel Passages, Ancient Literature, Biblical People/Places, plus media/sermons/outlines; links to Bible Facts (people, places, events, things). Wiki also lists Collections, Topics, Interesting Words, Compare Versions | [PG](https://support.logos.com/hc/en-us/articles/360016462872-Study-a-Passage-Using-the-Passage-Guide), [wiki](https://community.logos.com/wiki/logos-user-wiki/guides/passage-guide/) |
| Exegetical Guide | Verse/range | Textual Variants, Grammars, Grammatical Constructions, Visualizations, Word by Word, Important Words, Commentaries, Ancient Literature | [EG](https://support.logos.com/hc/en-us/articles/360016462852-Study-a-Passage-with-the-Exegetical-Guide) |
| Bible Word Study | English or Greek/Hebrew/Aramaic word | English: Topic, Hebrew Words, Greek Words, Senses, Phrases, Textual Searches. Original: Lemma, Translation, Septuagint Translation, Root, Senses, Example Uses, Clause Participants, Preposition Use, Phrases, Lemma in Passage, Textual Searches, Case Frames | [BWS](https://support.logos.com/hc/en-us/articles/360016688811-Studying-Words-Using-the-Bible-Word-Study-Guide) |
| Topic Guide | Topic | Topic (definition + dictionary links + Factbook link + related topics), Related Verses, dictionaries, sermons, illustrations, journals; customizable on desktop; folded into Factbook on mobile | [Topic Guide](https://support.logos.com/hc/en-us/articles/360016688791-Topic-Guide), [Studying a topic](https://support.logos.com/hc/en-us/articles/28756571440141-Studying-a-Topic-with-Logos) |
| Important Passages (section) | Passage | Related passages *with stated relationship* (shared themes, cultural concepts, parallels); all tiers | [Important Passages](https://support.logos.com/hc/en-us/articles/360018488931-Important-Passages-Guide-Section) |

**Factbook:** starting point for topic/person/place/event/passage; summary → Key Article → Dictionaries (library dictionary hits) → passages/resources. [Factbook](https://support.logos.com/hc/en-us/articles/360016146691-What-can-I-do-with-the-Factbook)

## 5. Cross-references, Text Comparison, Information, hover

- **Cross references** surface in: Passage Guide section (aggregated from library), standalone guide section, and Insights panel "Related Passages → Cross References" card drawn from owned Bibles; hover previews, click opens. Insights on desktop/mobile requires **Premium+**; web available to all. [Cross Refs](https://support.logos.com/hc/en-us/articles/360018252931-Cross-References-Guide-Section), [Insights](https://support.logos.com/hc/en-us/articles/25352521268109-Using-the-Insights-Panel)
- **Text Comparison:** Tools → Text Comparison; enter passage; click version names to choose; "Show differences" highlights wording differing from base text; compare Top Bibles or a collection; **available to all users**. [Text Comparison](https://support.logos.com/hc/en-us/articles/360015518292-Compare-Translations-with-Text-Comparison), [Three tools](https://support.logos.com/hc/en-us/articles/360054863171-Three-Tools-to-Compare-Bible-Translations)
- **Information tool:** separate panel showing details on word under cursor; "Update on: Hover | Click". Separate program setting for small info tooltips. [Information](https://support.logos.com/hc/en-us/articles/360015718071-Information-Tool), [Settings](https://support.logos.com/hc/en-us/articles/360015926432-Adjust-the-Logos-Program-Settings)
- **Scripture reference hover:** hovering a linked reference shows passage popup; click opens in top Bible. [Free start](https://support.logos.com/hc/en-us/articles/360023029051-Get-started-with-Logos-for-free)
- **Strong's:** via interlinear view / Reverse Interlinear pane in compatible Bibles; hover linked Strong's number previews lexicon entry, click opens. [Strong's](https://support.logos.com/hc/en-us/articles/360028307111-How-to-Access-Strong-s-Numbers)

## 6. Notes, highlights, notebooks

- Notes tool holds notes **and** an entry per highlight. Notebooks organize both; membership optional. [Notes](https://support.logos.com/hc/en-us/articles/360017978372-Record-Your-Insights-Using-Notes)
- Highlighting tool: **palettes** contain **styles**; palette can be assigned to a notebook (affects future highlights only). [Highlights](https://support.logos.com/hc/en-us/articles/360016517891-Annotate-Your-Books-with-Highlights)
- **Anchors:** note anchored to resource location or Bible reference; multiple anchors per note. [Anchors](https://support.logos.com/hc/en-us/articles/360031314832-How-Do-I-Add-and-Manage-Note-Anchors)
- Per-resource display filter: choose notebooks shown, "No notebook", and "From other books with corresponding text" (Bible-reference-anchored items appear in other translations). [Highlight & notes](https://support.logos.com/hc/en-us/articles/360036147972-4-Use-Logos-to-Highlight-Text-and-Take-Notes)
- Implication: two anchor kinds — **text-range anchor** (resource-specific offset) vs **reference anchor** (version-independent).

## 7. Search

| Kind | Scope | Notes | Source |
|---|---|---|---|
| Books (was "Basic") | Whole library / collection | words & phrases | [Search](https://support.logos.com/hc/en-us/articles/360039621591-How-Do-I-Search-in-Logos) |
| Bible | Selected Bibles + passage range | words, phrases, tagged info | [Bible search](https://support.logos.com/hc/en-us/articles/360017808111-How-to-Search-Your-Bible-Using-Logos) |
| Morph | Morphologically tagged texts | lemma, tense, mood etc. | [Morph](https://support.logos.com/hc/en-us/articles/360017524952-Morph-Search) |
| Syntax | Syntactically tagged originals | desktop-only | [Syntax](https://support.logos.com/hc/en-us/articles/360017808131-Greek-Syntax-Search) |

Bible search **result views**: Passages (matching verses, add versions side by side), Aligned (aligned on hit terms), Grid (distribution chart across Bibles/books), Analysis (tabular, original-language context). Grid and Aligned not in web app. [Bible search](https://support.logos.com/hc/en-us/articles/360017808111-How-to-Search-Your-Bible-Using-Logos). Search templates exist to build queries. [Templates](https://support.logos.com/hc/en-us/articles/360030747891-How-do-I-use-Search-Templates-to-Build-a-Search)

## 8. Reading plans

Documents → New → Reading Plan (or from a book/Library). Content: whole Bible/book, selected passages, or book portion. Pace: every day / weekdays / chosen days / no schedule. Split by finish date, minutes per day, or number of sessions. Custom per-session plans on desktop. Plans sync across platforms. [Reading Plans](https://support.logos.com/hc/en-us/articles/360016525132-Work-Through-Your-Books-with-Reading-Plans)

## 9. Tiers — what Logos treats as entry-level

- **Free:** "small library of Bibles, interlinears, commentaries, and dictionaries" (25+ resources) and "simple set of Bible study tools". [Free edition](https://www.logos.com/free-edition). Help Center lists free access to library, Bible/book search, notes, highlights, reading plans, link sets; Passage Guide, Bible Word Study (features vary by tier), Factbook; requires sign-in with free account. [Free start](https://support.logos.com/hc/en-us/articles/360023029051-Get-started-with-Logos-for-free), [BWS](https://support.logos.com/hc/en-us/articles/360016688811-Studying-Words-Using-the-Bible-Word-Study-Guide)
- **Premium** (~$8.33/mo annual): 200+ books, AI search/summaries, Insights. **Pro**: +200 books, sermon tools, translation, "Study Greek and Hebrew words". **Max**: +100 books, original-language grammar/syntax. [Pricing](https://www.logos.com/logos-explore-pricing) (prices fetched directly; may change).
- Feature/platform matrix: [Platform Comparison](https://support.logos.com/hc/en-us/articles/9785956686349-Logos-Platform-Comparison).

## 10. Feature table

| Feature | What it does | Data it needs | Lay student |
|---|---|---|---|
| Panels/tabs/docking | Multi-resource workspace | none (UI state) | Table stakes |
| Saved layouts | Restore workspace + link sets | serialized UI state | Table stakes |
| Float to 2nd monitor | Undocked windows | multi-viewport support | Advanced (nice) |
| Reference box nav | Type ref, jump; prev/next unit | reference parser, book-name aliases, resource index | Table stakes |
| Link sets | Panels follow same location | canonical reference index per resource; versification maps | Table stakes |
| Parallel translations (side by side) | Read multiple Bibles together | several Bibles keyed by verse | Table stakes |
| Parallel-resource cycling | Flip between books at same location | collections + per-resource index | Advanced |
| Library + prioritization | Choose default Bible/commentary/lexicon | resource metadata (type, lang, datatype) | Table stakes (simple) |
| Collections | Saved rule/hand-picked book sets for scope | metadata queries | Advanced |
| Text Comparison | Diff verse text across versions | ≥2 Bibles, verse-aligned; word diff | Table stakes |
| Cross-reference pane | Related verses for current verse | cross-ref dataset (e.g., TSK, public domain) | Table stakes |
| Passage Guide | Aggregate commentaries, xrefs, parallels, people/places for passage | verse-indexed commentaries, xrefs, parallel-passage/harmony data, entity tags | Table stakes (subset) |
| Exegetical Guide | Word-by-word grammar, variants, grammars | morph-tagged originals, apparatus, grammars | Advanced |
| Bible Word Study (Strong's level) | Lemma, gloss, occurrences, translation spread | Strong's-tagged English Bible + lexicon (Strong's/BDB/Thayer PD) | Table stakes |
| BWS senses/clause/case frames | Semantic analysis | proprietary sense/syntax datasets | Advanced (likely infeasible PD) |
| Topic Guide | Topic → verses + dictionary hits | topical index (Nave's PD), dictionaries | Table stakes (basic) |
| Factbook | Entity hub (people/places/events) | entity KB + verse links + dictionaries | Advanced (data-heavy) |
| Notes + notebooks | Personal notes with anchors | local store; reference & offset anchors | Table stakes |
| Highlights + palettes | Styled highlighting, cross-version | style definitions; anchor model | Table stakes |
| Books search | Full-text across library | full-text index | Table stakes |
| Bible search (+Passages view) | Verse hits across Bibles/range | per-verse index | Table stakes |
| Grid / Aligned / Analysis views | Hit distribution, aligned, tabular | same + tagging | Advanced (Grid modest) |
| Morph search | Grammar-form queries | morph-tagged originals (e.g., open morph data) | Advanced |
| Syntax search | Syntax-tree queries | syntax trees | Advanced |
| Reading plans | Scheduled readings, progress | reference ranges, local schedule | Table stakes |
| Hover ref popups | Preview verse on hover | reference detection in text | Table stakes |
| Information panel | Details on hovered word | Strong's/morph tags, lexicon | Table stakes (Strong's-level) |
| Insights / AI | Summaries, related cards | proprietary + online | Out of scope (offline, paid) |

## Implications for our decisions (facts, not decisions)

- **Engine model:** Logos's sync relies on per-resource **reference indexes + datatypes + verse maps**. Engine likely needs canonical reference type, versification-mapping layer, and per-resource "which datatypes I'm indexed by" metadata — pure data, WASM-clean.
- **Anchors:** two anchor kinds (reference vs text offset) affect format design: offsets must survive resource updates.
- **Guides are generic:** key input → ordered list of pluggable sections, persisted arrangement. Suggests section-trait in engine, renderer in ui-egui.
- **Data, not UI, is the bottleneck:** senses, case frames, Factbook entities, syntax, Insights depend on proprietary Faithlife data. PD analogues exist for Strong's, TSK, Nave's, Easton's, KJV-era commentaries (sourcing = separate ticket).
- **Logos free tier** includes guides, notes, highlights, search, reading plans, link sets → these are the entry-level bar. Logos free needs an account; our offline/no-account stance is a differentiator.
- **egui fit:** docking/tabs/float (multi-viewport) are needed table stakes; egui_dock-style crate vs custom is a separate decision. Grid/Aligned views and hover popups are standard egui patterns.
- **Clean-room:** section names above are descriptive; choose our own names/iconography to avoid trade-dress overlap.

## Sources

- Kept: Logos Help Center articles (support.logos.com, linked inline) — first-party behaviour docs; **indexed, direct fetch 403**.
- Kept: logos.com/free-edition, logos.com/logos-explore-pricing, logos.com/configure/subscriptions — fetched directly; tier facts.
- Kept (lower trust): community.logos.com wiki/forum (datatypes, verse maps, PG sections) — community-authored; indexed only.
- Dropped: community forum opinions on free version; de.logos.com Basic page (German duplicate); old Libronix PDFs (stale).

## Gaps

- Full text of Help Center pages not directly read (403). Verify section lists/menu labels manually or enable fetch fallback (Jina) per tool hint.
- Exact free-vs-paid per-section gating of BWS/Passage Guide not enumerated; Platform Comparison chart only partially indexed.
- Information panel contents, hover popup customization, and highlight style properties (colors, underline, icons) not detailed.
- Logos internal datatype list (official) not found; only community wiki.
