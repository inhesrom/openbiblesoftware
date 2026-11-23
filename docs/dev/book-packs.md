# Book packs (#415)

A **Book pack** is a named group of related Books in the Open catalogue, such as "The Puritans"
or "Confessions & Catechisms", that Get Resources downloads as one job. Packs are written in
`crates/engine/data/packs.toml` and read by `obs_engine::packs`.

## Rules

A pack is a rule over the catalogue's Books, not a list of ids, so it grows when a source
collection grows (ADR 0024). The file's header explains the rule. In short, a Book is in a
pack when it matches **any** inclusion:
- `all`;
- a `subjects` heading;
- an author in `authors`;
- a word in its title (`titles`);
- an entry id in `ids`.

It must also pass **every** restriction given:
- `collections`: EEBO-TCP, Evans-TCP, ECCO-TCP or Gutenberg;
- `languages`;
- `printed = [from, to]`: the edition's year;
- `died = [from, to]`: the last author's, translator's or editor's death year.

What a rule reads (`OpenSource`):
- **subjects:** the publisher's catalogue subject headings. TCP's come from `TCP.csv`'s
  `Terms`; Project Gutenberg's are its `Subjects` and `Bookshelves`. The xtasks write them
  into the `subjects` column of `tcp-books.tsv` and `pg-books.tsv`. Hand-written TCP entries
  carry `subjects` in `open-catalogue.toml`.
- **author:** as shown (`display_author`: "Owen, John, 1616-1683." is "John Owen").
- **printed:** `printed_year`, from the entry's date.
- **died:** `death_year` of a TCP author heading, or PG's `died` column.
- **collection:** `collection_name`.

## The packs

| Pack | Books | Download |
|---|---|---|
| Confessions & Catechisms | 68 | 24 MB |
| The Puritans | 711 | 480 MB |
| Sermons | 705 | 394 MB |
| Devotion & Prayer | 390 | 202 MB |
| The Reformation | 276 | 225 MB |
| Early America | 363 | 128 MB |
| The 1800s | 235 | 199 MB |
| Philosophy & Classics | 158 | 98 MB |
| All Books | 2,818 | 1.9 GB |

(`cargo test -p obs-engine --lib print_pack_sizes -- --ignored --nocapture` prints them.)

## Download

Get Resources → **Books** starts with a **Book packs** section (`get_bibles_tab::pack_row`).
Each row shows:
- the pack's title and description;
- how many Books it has, how many are in the Library, and the size left to download;
- **Download pack**, its progress with **Cancel**, or "In your Library".

`Session::pack_summaries` makes the rows. Membership is cached per catalogue
(`GetBibles::pack_members`), so a frame costs a pass over the Import records.

**Download pack** runs `getbibles.download_pack` (`Session::download_pack`):
- **Asking first:** All Books, or a pack with 250 MB or more left (`CONFIRM_BYTES`), asks first
  (`Effect::ConfirmPack`; the window is `State::pack_prompt`). Its **Download** runs the
  command again with `confirm`.
- **One batch:** the Books of the pack not in the Library, and not already downloading, go
  through the book batch (`obs_engine::books`) as **one job with one Import report Tab**. A
  `PackStream` downloads one Book at a time through the app's transport:
  - a 500 ms pause between downloads, for Project Gutenberg's mirror;
  - up to three tries, with a longer wait each time;
  - each Book checked by its pin (`OpenSource::check_download`).

  The batch imports each Book with its entry's importer, licence and Attribution, as a single
  download would. It records the Book's `OpenOrigin`, so its row says it is in the Library.
- **As each Book arrives:** it joins the Library, and a Library Collection named for the pack
  (`Session::add_to_collection`).
- **Failures:** a Book that cannot be downloaded or is refused by its pin is listed in the
  report with why, and the rest go on.
- **Cancel** (`import.cancel` with the batch's report id) stops between Books; those already
  in stay.
- **Resuming:** running the pack again fetches only what is missing.

Not yet:
- Remove pack;
- Update all for Open catalogue Books;
- a free-space check before a large pack. No crate in the tree reads free space without
  `unsafe`; the confirmation gives the size.

## Tests

`crates/engine/src/packs_tests.rs` checks that:
- every pack is sound and not empty;
- every author a pack names writes at least one of its Books, which catches a misspelt name;
- "All Books" is every Book and nothing else;
- the packs hold what their names promise;
- inclusions combine with *any* and restrictions with *all*;
- `a_pack_downloads_in_one_job_each_book_checked_and_collected`, offline: real TCP files are
  served at `file://` URLs with their real pins. It checks one job and one report Tab, a
  changed file refused while the rest go on, each Book installed as a single download leaves
  it and added to the pack's Collection, a second run fetching only what is missing, and All
  Books asking first.

Live, by hand: `cargo test -p openbible --release -- --ignored --nocapture
live_a_book_pack_downloads_in_one_job` downloads Confessions & Catechisms (68 Books, 24 MB).
