# Christian Open Library books download through the user's own GitHub access, on request

The Christian Open Library (COL) keeps its 7,600 works as Markdown in a private GitHub
repository, `Christian-open-library/books`, that a person reads after COL invites their GitHub
account (#342). Users want those works in their Library at the click of a button and kept up to
date (#347), without cloning a 3 GB repository and importing it by hand. The app has had one
network request of its own, the Update check (ADR 0017), and that ADR says any other needs its
own decision. We decided:

1. **COL books come from the user's own access, never from us.** Get Resources' **Books**
   filter (the Christian Open Library) reads the repository in one of two ways the user
   chooses:
   - **GitHub, with a token the user pastes**: a fine-grained personal access token with
     read-only Contents on `Christian-open-library/books` (or a classic token with `repo`
     scope, which can also write the user's own private repositories; the dialog says so). The
     app reads the repository with GitHub's REST API; or
   - **a local clone** the user keeps with git or GitHub Desktop: the app reads the folder and
     needs no sign-in and no network.
   Nothing from COL is bundled, mirrored, cached for others or uploaded. Downloaded books go
   only into this user's Library, as any import does.
2. **The token is kept in the operating system's credential store**, never by the app: Keychain
   on macOS, Credential Manager on Windows, the Secret Service on Linux (`keyring`, service
   `OpenBible`, account `github-token`), behind the engine's `SecretStore` trait. It is never
   written to the User database, settings, Layouts, the Last session, logs or Crash reports, and
   no error message carries it. Sign out deletes it. Where no credential store answers (a Linux
   desktop without a Secret Service), the app keeps the token for this session only and says so.
   It is sent only to `https://api.github.com/` as `Authorization: Bearer …`; the tarball's
   redirect to `codeload.github.com` carries its own short-lived URL and no token (the transport
   never forwards an Authorization header to another host).
3. **Every request is a user action.** With no click the app makes no request to GitHub for COL.
   Opening the Books filter once connected reads the repository's tree and its
   `catalog_full.csv` and `authors.csv` (three requests); Download fetches the chosen books;
   Check for new books reads the tree again. Before connecting, "Browse the catalogue" reads
   COL's public `catalog.csv` once. There is no background check; the Update check (ADR 0017)
   is still the only request the app makes by itself.
4. **What is downloaded.** One work, a selection or a filter's whole result is fetched file by
   file by blob SHA (`GET /repos/…/git/blobs/{sha}`, raw), so what is imported is exactly the
   blob the tree named. More than 400 files, or Download everything, is one
   `GET /repos/…/tarball/{commit}` streamed through gzip and tar into the importer, a folder at
   a time, never unpacked to disk whole. Either way each book becomes a Book (ADR 0023) through
   the book batch of #344, under one Import report.
5. **A book's Resource id comes from its path**: `col-` + the slug of its file name (without
   `— Part N`) + `-` + the first 6 hex digits of the sha256 of its folder and stem, at most 64
   characters. It is the same on every computer and every download, so an Annotation made on
   one copy re-anchors on the next revision.
6. **Updates.** Each COL Resource's Import record keeps a `ColOrigin`: the repository, the
   book's main path, the commit it came from and the blob SHA of each of its files. Check for
   new books reads the tree, compares those SHAs, and lists books **new**, **changed** and
   **no longer in COL**. Update writes a changed book as its next Resource revision under the
   same id (ADR 0006 re-anchors its Annotations); a removed book stays in the Library, marked.
   New books are offered, never downloaded unasked. A local clone is compared by the same git
   blob SHAs, computed from the files.

## Considered Options

- **GitHub OAuth device flow:** the smoothest sign-in, but it needs an OAuth App registered
  under the maintainer's account with its client id compiled in, and asking for the `repo`
  scope grants write access to every private repository the user has. It can be added later
  as another way to get the token.
- **Keep the token in the User database, encrypted:** the key would sit beside it on the
  same disk; the credential store is what each platform provides for this.
- **Ask COL for a download API or bundle:** COL's terms keep the collection behind each
  person's own membership; reading through the user's own access is what a member can do.
- **Contents API by path instead of blobs by SHA:** paths hold spaces, em dashes and old
  spellings that must be encoded exactly; a blob SHA names the exact bytes the tree listed.
- **Clone with git inside the app:** a git implementation and its own credential handling in
  the app, for what three REST calls and a tarball do.

## Consequences

- CONTEXT.md names the COL download as a second network use, on user action only, beside the
  Update check.
- The transport gains `Fetch::get_with` (a GET with request headers) and `Fetch::open` (the
  same, the body handed over as it arrives) and `FetchError::Unauthorized`; an Authorization
  header is sent only to GitHub's API (or localhost, for tests) and never follows a redirect.
  The engine still opens no connection itself, and tests use a mock repository.
- `keyring` (MIT OR Apache-2.0) joins the desktop app; on Linux it uses the zbus the app
  already links.
- COL's commentaries (359 works) come in as Books, not Passage-keyed Commentaries, until their
  chapter and verse headings are parsed (a later ticket).
- If COL renames a file, the book's id changes with it: the old Resource stays, marked no
  longer in COL, and the new path comes in as a new Book.
