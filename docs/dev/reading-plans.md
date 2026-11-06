# Reading plans (#147)

Tools → Reading plans makes a **Reading plan** from a **Reading plan template**, with a start
day, a Bible and the days of the week to read on; **Today's reading** on the toolbar opens the
day's **Reading** in one click; Readings are ticked off; a plan that is **Behind** offers
**Catch up** and **Reschedule**. Terms are in [CONTEXT.md](../../CONTEXT.md).

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `reading_plans.rs`, `reading_plans_store.rs` | User database schema 8: `reading_plan` (name, template id, Bible, start day, weekday mask) and `plan_reading` (position, Passages as a Standard-versification Bible reference, due day `YYYY-MM-DD`, `read_at`). Deleting a plan cascades to its Readings. |
| `obs-engine` | `reading_plans.rs` | `PlanDate` (civil days, no time-zone crate), `Weekdays`, `PlanTemplate` (loaded from `assets/reading-plans/*.toml`), scheduling, `ReadingPlan` (`due_on`, `overdue`, `behind_days`, `todays_reading`, `catch_up`, `reschedule`), `Session::reading_plans`, `toolbar_plan`, `plan_today`; commands `tool.reading_plans`, `reading_plan.create`, `.mark`, `.open`, `.catch_up`, `.reschedule`, `.remove`. |
| `obs-ui-egui` | `shell/reading_plans_tab.rs` | The Reading plans Tool (New plan form, a card per plan with Today, Missed and Next, the Behind bar) and Today's reading on the toolbar, left of the Command box. |

## Templates

Each template is a TOML file in `crates/engine/assets/reading-plans/`, credited through
`ATTRIBUTION.toml` (so it shows in Help → Credits under data built into the app):

- `tracks`: lists of Bible references read in order. Each track is cut into whole-chapter
  units (a Passage's partial first or last chapter stays partial), weighted by Verse count,
  and divided greedily into one run per reading day of about equal weight; adjacent units in
  a run join into one Passage. With fewer chapters than days (a custom "Jude over 5 days"),
  the units are single Verses instead. Each track gives one Reading a day, so Psalms and
  Proverbs monthly has two.
- `readings`: each day's Readings as the template gives them (M'Cheyne: four a day).

| Template | Days | Per day (Verses, Every day) | First Readings |
|---|---|---|---|
| Bible in a year (canonical) | 365 | 53–176, mean 85 | Gen 1-3, Gen 4-6, Gen 7-9 |
| Bible in a year (chronological) | 365 | 29–176, mean 85 | Gen 1-3 … Gen 10-11; Job 1 |
| Bible in a year (M'Cheyne) | 365 | 65–188, mean 113 | Gen 1, Matt 1, Ezra 1, Acts 1 |
| New Testament in 90 days | 90 | 67–118, mean 88 | Matt 1-4, Matt 5-6 |
| Psalms and Proverbs monthly | 31 | 79–204, mean 108 | Ps 1-8 and Prov 1 |

A custom plan (`template: "custom"`) reads `books` (any Bible reference, e.g. `Rom-Gal; Jas`)
over `days` days, at most `MAX_CUSTOM_DAYS` (3650, ten years; scheduling lays out every day, so
a larger number is refused before anything is laid out). Where the templates come from, and the licence check of chronological
orders, is in [docs/research/reading-plan-templates.md](../research/reading-plan-templates.md).

## Laying a plan out

`PlanTemplate::schedule(start, weekdays)`: the reading days are the days in
`[start, start + days)` whose weekday is chosen, so choosing fewer weekdays keeps the plan to
its span (a year stays a year) and gives each reading day more. Track templates divide their
tracks into exactly that many runs; `readings` templates spread their days evenly over the
reading days (day *j* of *L* goes on reading day ⌊*j*·*K*/*L*⌋ of *K*), keeping each day's
Readings together. Every Reading is stored with its due day, so later moves are plain
updates.

## Today, Behind, Catch up and Reschedule

Today is the local day as SQLite's `localtime` has it (`UserData::local_day`, the same clock
History groups by), checked again with each Last session save.

- **Due today**: Readings whose day is today. **Missed**: unread Readings before today.
- **Behind by N days**: N distinct days before today with an unread Reading.
- **Catch up** (`reading_plan.catch_up`): the unread Readings, grouped by their day, all move
  on by the same number of reading days, so the first lands on the first reading day from
  today and the gaps between them stay (a sparse plan stays sparse); the plan ends that many
  reading days later.
- **Reschedule** (`reading_plan.reschedule`): the plan keeps its last day; the unread
  Readings, grouped by their day and in order, are spread evenly over the reading days from
  today to the last day, the first group today and the last on the last day (several on a
  day when there are more groups than days). With no reading day left before the end it
  catches up instead.
- Neither moves anything when the plan is not behind; read Readings never move.

`Today's reading` (`Session::toolbar_plan`) shows the first plan with an unread Reading due
today or missed (else the first unfinished one): "Today: Gen 1-3" opens the first unread
Reading in the plan's Bible (`reading_plan.open`: its most recent Tab of that Bible moves
there, else a new Tab opens; a Bible no longer in the library gives way to any Bible),
"Behind by N days" opens the Tool, "✓ Today's reading" once today's are read.

## Hand-offs

- Home (#178) lists each unfinished plan's Readings due today (`Session::home_plans`), each
  opening with `reading_plan.open {plan, position}`; see [home.md](home.md).
- The Command box (#139) can run `reading_plan.open` with a plan id for "Today's reading".
