# Home (#178)

**Home** is the Tab that gathers what a reader comes back to and what a new user can start
with. It opens from the menu bar's **Home** button (left of Today's reading) or Alt+H
(`home.open`), and at launch on the first run. Terms are in [CONTEXT.md](../../CONTEXT.md).

| Layer | Piece | Role |
|---|---|---|
| `obs-engine` | `home.rs` | `TabKind::Home` (keyless, single instance, no Link set); `HomeCard`, `HomeSettings` (the Home arrangement and "Open Home at launch", one preference `home` in the User database); what each card lists (`Session::home_plans`, `home_continue`, `home_history`, `home_favorites`, `verse_of_the_day`, `quick_start_params`, `library_summary`, `starter_summary`); commands `home.open`, `home.read_verse_of_the_day`, `home.move_card`, `home.hide_card`, `home.reset_cards`, `home.open_at_launch`. |
| `obs-engine` | `workspace.rs` | A new Home Tab goes to the last reading Panel (over the ★ Bible) unless a Panel is remembered for it, so it has room; focusing Home makes its Panel the last reading Panel, never the last non-reading one, so Tools and the Starter library still open beside it (`TabKind::opens_in_reading_panel`). Home is not Bible content (`is_reading` stays false). |
| `obs-ui-egui` | `shell/home_tab.rs` | The cards (two columns when the Tab is at least 760 points wide), their headers (⏶ ⏷ ×), the line under the title (Open Home at launch, Reset cards, Show for each hidden card), and the menu bar's Home button. |
| `obs-ui-egui` | `shell.rs` | `Shell::restore_at_launch`: the Last session (else the default layout), then Home on top when there was no Last session or "Open Home at launch" is on. `App::with_file` calls it, then `Shell::show_startup_resource` for a file given on the command line: its open Tab (the ★ Bible of the default layout) is focused over Home, else it opens. |

## Cards

In their default order. Every row opens with one click through the command of the Tool it
comes from (Ctrl/Cmd-click: in a new split), so it behaves as it does there.

| Card | Shows | One click runs |
|---|---|---|
| Today's reading | Every unfinished Reading plan, none left out (Home scrolls): its name, "Behind by N days" when it is, and the Readings due today (none due and behind: the first missed one; nothing due: "Next reading: Mon 9 Nov"). With no plan: Start a Reading plan…. | `reading_plan.open {plan, position}` (the plan's Bible); Behind and Start open `tool.reading_plans` |
| Continue where you left off | The newest Visit in History whose Resource is still in the library, with its day and time. | `history.open {id}` |
| History and Favorites | The next five Visits, each place once; the first six Favorites; All History…, All Favorites…. | `history.open`, `favorite.open`, `tool.history`, `tool.favorites` |
| Verse of the day | Today's Passage from `VERSES_OF_THE_DAY` and its text in the ★ Bible (the same preview as a reference's hover). | `home.read_verse_of_the_day {split}`: always the ★ Bible (its most recent Tab, else a new one), never another open Bible (`Session::show_passage_in`, shared with `reading_plan.open`) |
| Quick start | Passage Guide, Word Study, Get Resources, Import, each with what it is for. | `guide.passage_guide {passage}` (at the most recent Bible Tab's Passage, else the Verse of the day), `guide.word_study`, `library.get_resources`, `library.import` |
| Your library | How many Resources of each Resource type; an empty type links to Get Resources at that type (Cross-reference sets and People & places indexes, which the repositories do not offer, to the Starter library); "Starter library: N Resources" (N of M when some are missing) or Add the Starter library…; Open the Library. | `library.get_resources {type}`, `library.starter`, `library.open` |

Everything comes from the library and the User database, so Home works offline; only Get
Resources goes online, when clicked.

## The Home arrangement

Like a Guide arrangement: the order of the cards and which are hidden, stored as
`{"cards":[{"id":"quick_start","hidden":false},…],"open_at_launch":false}`. Unknown ids are
dropped and cards a save does not mention go to their default place, so a later version can
add cards. ⏶ and ⏷ in a card's header move it past the shown card before or after it
(`home.move_card {card, to}`, `to` counting hidden cards too); × hides it
(`home.hide_card {card}`); Show under the title brings it back (`hidden: false`); Reset cards
puts every card back, shown, in the default order (`home.reset_cards`; Open Home at launch is
kept). The buttons are keyboard-reachable and named ("Move Quick start up", "Hide Verse of the
day"); Guides reorder by dragging their headers, Home by buttons because its cards sit in two
columns.

## Verse of the day

`VERSES_OF_THE_DAY` is our own list of 51 well-known Passages of one to four Verses (Standard
versification references only, no Bible text, so no licence question). The Passage on a day
is `VERSES_OF_THE_DAY[day mod 51]`, where `day` is the local day as Reading plans count it
(days since 1970-01-01, `Session::plan_today`): the same all day, a different one the next day,
round the whole list every 51 days. The text is read from the ★ Bible; one that lacks the
Passage says so.

## At launch

- **First run** (no Last session): the default layout, then Home over the ★ Bible, focused.
  When the Starter library is offered for download (no bundle), its Tab opens beside it,
  in the non-reading Panel, and Home stays shown.
- **Later launches**: the Last session as left. Home is there only if it was open, unless
  **Open Home at launch** is on (the checkbox under Home's title, `home.open_at_launch`):
  then Home opens on top of the Last session.
- **A file given on the command line** shows on top of both: its Tab is focused if open
  (the ★ Bible on a first run), else it opens.

## Evidence

- `crates/engine/src/home_tests.rs`: the Tab and its placement (a Tool still opens beside a focused Home; with no Bible, the
  Starter library offer opens beside Home), the arrangement kept after a
  restart, the stored form, the Verse of the day (read in the ★ Bible even when only another
  Bible is open), the fresh-install summary, every unfinished Reading plan listed (three
  starting later do not hide a fourth due today), and one command per row opening a Reading,
  a Visit and a Favorite.
- `crates/ui-egui/src/home_tests.rs` (headless harness, real `.obr` files and User database):
  a fresh install opens on Home with Quick start and the Starter library summary; a Reading
  plan, a Favorite and Visits each open with one click; the Verse of the day opens the ★ Bible
  (BSB) with only WEB open; a fourth plan after three starting later is listed and opens;
  ⏶, × and Show; Open Home at launch across a relaunch; without a bundle the Starter library
  offer opens beside Home with Home still shown; a startup file that is the ★ Bible is
  focused over Home, one not open yet opens over it.
- `crates/ui-egui/src/a11y_audit_tests.rs`: Home in every-Tab-kind naming and contrast, and
  `keyboard_only_home`.
- Real app (macOS, `OBS_SCRIPT` + `OBS_SHOT`, fresh User database): `docs/previews/home-first-run.png`
  (first launch), `docs/previews/home-reading-plan-and-favorites.png` (after
  `reading_plan.create`, `resource.open`, `tab.go`, `favorite.add`, `home.open`).
- Headless previews: `docs/previews/home-fresh-install-kittest.png`,
  `docs/previews/home-reading-plan-and-favorites-kittest.png` (`UPDATE_PREVIEWS=1 cargo test -p
  obs-ui-egui --lib home::`, converted from `target/previews/*.ppm` with ffmpeg).
