# The Christian Open Library download (#347)

Get Resources' **Books** filter downloads the Christian Open Library's (COL) books into the
Library as Books (ADR 0023) through the user's own access ([ADR 0024](../adr/0024-christian-open-library-books-download-through-the-users-own-github-access.md)).

## Engine

- `obs_engine::col` (no network): `ColTree::from_github` reads GitHub's recursive tree, keeping
  only `wanted_file`s (Markdown and the two catalogue files) with `safe_path`s, regular modes,
  hex blob SHAs and sizes under the importer's cap; links, `..`, `.git`, absolute paths and
  oversized files are refused and counted. `works` joins the tree's books (`group_book_files`)
  with `catalog_full.csv` (author, title, year, type, licence, source) and `authors.csv`
  (tradition); a book the catalogue does not list is still offered by its file name. CSV is read
  by hand (RFC 4180, bounded). `ColFilter` matches type, author, tradition, collection, years and
  words of title or author, folding case, accents, ſ/s, u/v and i/j. `col_resource_id` gives each
  work the same Resource id everywhere: `col-<slug>-<6 hex of sha256(folder/stem)>`.
  `compare` sets the Library's COL Books (their `ColOrigin`: path, commit, each file's blob SHA)
  against a tree: new, changed, removed, current. `git_blob_sha` hashes a local copy's files the
  way git does.
- `obs_engine::col_get`: `ColState` on the Session (`session.col`), the `col.*` commands and the
  three `BookStream`s a book batch reads:
  - `BlobStream`: each file by `GET /repos/…/git/blobs/{sha}` (raw), checked against its SHA;
  - `TarStream`: `GET /repos/…/tarball/{commit}` through `Fetch::open`, gunzipped and untarred
    on a reader thread, each book handed over through a two-slot channel as soon as every one of
    its files has passed (git orders entries by path, so a book's ` — Part N` files come before
    its main `.md`); each file is checked against the tree's blob SHA. Used for everything, and
    for more than 400 files;
  - `LocalStream`: a local copy's files.
  Every book is written as its `col_resource_id`: a book already in the Library becomes its next
  Resource revision (Annotations re-anchor), and its Import record keeps its `ColOrigin`. The
  Attribution gains "From the Christian Open Library (…), its copy <path>."
- Token: `Token` never prints itself; `SecretStore` keeps it (`SessionSecret` by default and in
  tests, the OS keychain in the app). A 401 forgets it. The local copy's path is the only COL
  value in the User database (`col/local-clone`).
- Errors: `ColError` names no access (404), a refused token (401), rate limits (403 with no
  requests left, 429), offline, and a folder that is not a copy, in plain words.

## Commands

| Command | Does |
|---|---|
| `col.connect_token {token}` | keep the token, read the repository (repository, branch's commit, tree, catalogue, authors) |
| `col.connect_local {path}` | read a local copy (no network) |
| `col.disconnect` | forget the token or folder |
| `col.refresh` | Check for new books |
| `col.browse_public` | read COL's public `catalog.csv` |
| `col.filter {text, kind, author, tradition, collection, year_from, year_to}` | |
| `col.select {paths?, on?}` | choose works (every shown one without `paths`) |
| `col.download {scope: selected\|shown\|everything, paths?}` | one book batch |
| `col.update {ids?}` | update the changed COL Books |

## App

`apps/openbible/src/fetch.rs`: `Fetch::get_with` and `Fetch::open` send headers, refuse an
`Authorization` header to any host but `https://api.github.com/` (or localhost for tests), and
never forward it across a redirect (`RedirectAuthHeaders::Never`), so the tarball's redirect to
GitHub's download host goes without it. `apps/openbible/src/secrets.rs`: `OsKeychain` (`keyring`,
service `OpenBible`, account `github-token`).

## UI

`crates/ui-egui/src/shell/col_tab.rs`, under Get Resources' Books chip: Connect (token, local
copy, Browse the catalogue), then the connection line with Check for new books and Disconnect,
the changes line with Update N, the filters, the chosen count and size, Download chosen / all
shown / everything… (asks first), and one row per work (Download, or Open with In your Library
or Changed in COL).

## Tests

- `crates/engine/src/col_tests.rs`: tree safety, works and ids, filters and old spellings, CSV,
  git blob SHAs, the comparison.
- `apps/openbible/src/col_tests.rs`: end to end over HTTP on localhost with the app's own
  transport: connect, filter, download two works file by file, everything by tarball through a
  302 to another host (which never sees the token), a Note on a book that COL then changes,
  Check for new books, Update in place with the Note re-anchored, the token absent from the User
  database; refused (401), no access (404), rate limited (429) and offline.
- `crates/ui-egui/src/col_tab_tests.rs`: the Books view through real clicks against
  `testing::MockCol`; the gallery shows it not connected, connected and with no access.
- A real download against COL's repository needs a member's token: run the app, paste it under
  Get Resources → Books, and download a work.

## A manual run

The real app (Linux, under Xvfb) against a mock of COL's repository on this computer
(`OBS_COL_API=http://127.0.0.1:<port>`, a Python server answering the same REST calls), driven
by `OBS_SCRIPT` and `OBS_STEPS`: `docs/previews/347-app-books-not-connected.png`,
`347-app-books-connected.png`, `347-app-one-book-downloaded.png` (the batch's Import report and
Home's Books 1) and `347-app-book-open.png` (the Book with its Contents, printed pages and
reference links).

## Search Christian Open Library (#343)

A link out, no download: `col.search_website {text}` opens `https://christianopenlibrary.org/#q=…`
(`col::website_search_url`: whitespace collapsed, cut at a word to 500 characters,
percent-encoded) in the system browser through `Effect::OpenUrl`. The search is after the `#`,
which the browser keeps to itself; COL's page reads it and its server never receives it. The
app's transport is not used. The right-click menu offers it for a Selection (the text) and a
Verse (its Standard reference); the Passage Guide's Cited in section ends with "Search the
Christian Open Library for <Passage>". Settings → Search → Search Christian Open Library
(`col.search_link {on}`, preference `col/search-link`, on by default) hides every entry point.
