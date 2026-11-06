# A format version that only adds a table keeps older files readable

`.obr` format 4 (#176) adds one table, `volumes` (a Commentary's volume per Bible book, for the Contents pane). Until now the reader opened exactly one format version (ADR 0003), so every bump made every installed `.obr` unreadable until it was imported or built again, and the tables added since format 3 (`renderings`, `strongs_totals`, `entity_strongs`) were added without a bump, as optional tables read as empty when absent. From format 4 on, the version says which tables a file has, and the reader opens a range: `OLDEST_FORMAT_VERSION` (3) to `FORMAT_VERSION` (4). A format 3 file reads with no volumes; a format 4 file always has the table (empty when the Resource names no volumes), and one without it is damaged. The builder writes only `FORMAT_VERSION`.

## Considered Options

- **No bump, optional table (the `renderings` precedent):** nothing to migrate, but a file's version no longer says what it holds, and an unchanged content hash gives the Starter build no new revision, so installed Starter copies would never get the table.
- **Bump and refuse older files (the format 2 → 3 precedent):** every installed `.obr` (Starter and imported) stops opening until it is built again, for a table only Commentaries with volumes use.
- **Bump and read a range (chosen):** the version is exact again and older files keep working. The content hash still covers the format version, so a format 4 build of unchanged text is a new revision: the Starter build gives it a higher `revision`, the app replaces the Starter copy (ADR 0007), and Annotations re-anchor as for any revision (ADR 0006), finding their quotes where they were.

## Consequences

- A bump that changes the meaning of existing data (span kinds, attrs, the hash input) still raises `OLDEST_FORMAT_VERSION` with it; only additive bumps keep older files readable. Each reader of an added table answers its empty value for files older than the version that added it.
- Every Resource's content hash changes once with the bump (golden test hashes too); text, spans and Keys do not.
