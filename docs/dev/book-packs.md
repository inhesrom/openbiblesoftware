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
| The Puritans | 710 | 480 MB |
| Sermons | 705 | 394 MB |
| Devotion & Prayer | 390 | 202 MB |
| The Reformation | 276 | 225 MB |
| Early America | 363 | 128 MB |
| The 1800s | 235 | 199 MB |
| Philosophy & Classics | 158 | 98 MB |
| All Books | 2,818 | 1.9 GB |

(`cargo test -p obs-engine --lib print_pack_sizes -- --ignored --nocapture` prints them.)

## Tests

`crates/engine/src/packs_tests.rs` checks that:
- every pack is sound and not empty;
- every author a pack names writes at least one of its Books, which catches a misspelt name;
- "All Books" is every Book and nothing else;
- the packs hold what their names promise;
- inclusions combine with *any* and restrictions with *all*.
