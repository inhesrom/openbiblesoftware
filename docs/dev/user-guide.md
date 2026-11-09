# User guide and First-run tour (#218)

**Help → User guide** (F1, `help.user_guide`) opens the offline guide bundled with the app in a
single-instance Tab; **Help → Take the tour** (`help.tour`) replays the **First-run tour**, a few
**Coach marks** that run by themselves at the first launch. Terms are in
[CONTEXT.md](../../CONTEXT.md).

| Layer | Piece | Role |
|---|---|---|
| `docs/user-guide/*.md` | the pages | One task-based page per main feature, written as Note formatting (CommonMark, ADR 0010) with the CONTEXT.md terms. Its first line is `# <title>`. Readable on GitHub too: links between pages and screenshots are relative. |
| `obs-engine` | `user_guide.rs` | `USER_GUIDE` (the pages, compiled in with `include_str!`), `guide_blocks` (a page's lines through `notes::note_body`, between its screenshots), `search_user_guide`, `page_for_link`; `TabKind::UserGuide` (single instance, opens with the reading Panels like Home, a search box, no Link set); commands `help.user_guide {page?}` and `user_guide.page {page}`. |
| `obs-engine` | `tour.rs` | `TOUR_STEPS` (six Coach marks and what each points at, `TourTarget`), `Tour` (the Coach mark showing; whether the tour was shown, the `tour` preference `{"shown":true}` in the User database), `Session::start_tour_at_first_launch`; commands `help.tour`, `tour.next`, `tour.back`, `tour.skip` (the last three only on the Coach mark: `IN_PLACE_COMMANDS`). |
| `obs-ui-egui` | `shell/user_guide_tab.rs` | The Tab: the page list (a row of links in a narrow Tab), the page drawn line by line by the Notes renderer (`State::body_line`, as a Note card's body), the screenshots (`GUIDE_IMAGES`, compiled in with `include_bytes!` and decoded once by tiny-skia), Next page, and the search results. |
| `obs-ui-egui` | `shell/notes_tab.rs` | `body_links`: a link to `<page>.md` shows that page (`user_guide.page`). |
| `obs-ui-egui` | `shell/tour_ui.rs` | Where each target was drawn this frame (`TourUi::target`; the Launchers from the toolbar's own rects), the ring and the card beside it, Esc. |
| `obs-ui-egui` | `shell.rs`, `lib.rs` | `Shell::restore_at_launch` starts the tour after Home; a scripted run (`OBS_SCRIPT`, `OBS_INPUT`, `OBS_STEPS`, `OBS_SHOT`) skips it unless `OBS_TOUR=1`. |

## The pages

Getting started, Read and compare, Link sets, Guides, Word Study, Search, Notes and Highlights,
Get Resources, Layouts, Keyboard. Every line is drawn as a Note's line is: headings in the
heading colour, lists, bold and italic, Bible references as live links (hover previews the
Verse, click opens it), web links open the browser, and `[Link sets](link-sets.md)` shows that
page. A line that is only `![description](../previews/<file>.png)` is a screenshot, drawn as
wide as the page (never wider than taken) with its description under it.

The header box searches every page: a page is listed when its title or text holds every word
typed (any case), pages whose title holds one first, each with up to three lines that hold one.
Clicking a result shows the page and clears the search.

## Screenshots

The screenshots are real-app shots in `docs/previews/` (the screenshot gallery), named
`user-guide-*.png`: 1280 × 800, Light theme, the Starter library, taken with `OBS_SCRIPT` +
`OBS_SHOT` and quantized to 192 colours (`magick in.png -dither None -colors 192 PNG8:out.png`)
so the ten add about 1 MB to the app. To retake one, keep its file name (or add it to
`GUIDE_IMAGES` and the page). When the screenshot test gallery (#222) lands, a page can point
at a gallery image the same way.

## Keeping the guide honest

| Test | Checks |
|---|---|
| `user_guide_tests::the_user_guide_has_a_page_for_every_main_feature` | the pages the ticket names exist, start with their title, are more than a stub, and are the files on disk; no file is left out |
| `user_guide_tests::every_guide_tool_and_launcher_is_named_in_the_user_guide` | every item of the Tools menu, every Launcher, Home, the Command box, the Starter library, Layouts and Link sets are named on some page |
| `user_guide_tests::every_menu_path_in_the_user_guide_names_a_real_item` | each `Menu → Item` written is an item of that menu (or the Guides Launcher) |
| `user_guide_tests::every_link_between_user_guide_pages_names_a_page`, `every_user_guide_screenshot_is_in_the_screenshot_gallery` | links and screenshots resolve |
| UI `user_guide::every_guide_screenshot_is_compiled_in_and_decodes` | each screenshot is compiled in, decodes and is used |

## The First-run tour

| Coach mark | Points at |
|---|---|
| The toolbar | the menu bar and the Launchers |
| Your Library | the Library Launcher |
| Link sets | the Link set chip of the focused Tab (a Bible's) |
| Passage Guide | the Guides Launcher |
| Word Study | a Bible's text (right-click a word) |
| Home | the Home button |

- **When it runs.** `Shell::restore_at_launch` runs it over Home when the User database has no
  `tour` preference saying it was shown. Starting it saves that preference at once, so closing
  the app during it does not bring it back. Without a User database it never starts by itself
  (nothing could remember it). Help → Take the tour replays it any time.
- **A card at a time.** Each Coach mark rings its target and sits under it (above it, or beside
  it, when there is no room), never over it. It does not block the window. A target not drawn
  this frame (the toolbar Auto-hidden or in its » button, no Tab with a Link set) puts the card
  in the middle of the window. The Link sets and Word Study Coach marks show the most recent
  Bible Tab when the focused Tab is not a Bible (Home at the first launch), so they have
  something to point at.
- **Buttons.** Next (Done on the last), Back from the second, Skip tour; Esc skips it unless a
  menu or pop-up is open.

## Evidence

- Real app on macOS (`OBS_SCRIPT` + `OBS_SHOT`, `OBS_TOUR=1`, the Starter library):
  [Help menu before](../previews/help-menu-before-218.png) and
  [after](../previews/help-menu-after-218.png); [first launch before](../previews/first-launch-before-218.png);
  the six Coach marks [1](../previews/tour-1-toolbar.png), [2](../previews/tour-2-library.png),
  [3](../previews/tour-3-link-sets.png), [4](../previews/tour-4-passage-guide.png),
  [5](../previews/tour-5-word-study.png), [6](../previews/tour-6-home.png); the User guide Tab
  [Getting started](../previews/user-guide-tab-getting-started.png) and
  [Word Study](../previews/user-guide-tab-word-study.png).
- Headless harness (`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui user_guide tour`):
  [search](../previews/user-guide-search-kittest.png),
  [Getting started](../previews/user-guide-getting-started-kittest.png),
  [Link sets Coach mark](../previews/tour-link-sets-kittest.png).
