# One SQLite file per Resource, with text stored as plain strings plus standoff spans

Each Resource is one read-only SQLite file (`.obr`) that holds its manifest, its text, its Keys, and (for a Bible) its verse list and Verse map. Text is stored as plain NFC strings, one per Bible book, commentary section, or lexicon or dictionary entry. Everything else is a separate layer of spans laid over that string by UTF-8 byte range: verses, paragraphs, poetry lines, headings, Tagged words, notes, words of Jesus, and so on. USX and OSIS both let verses and paragraphs overlap, so a nested-container model cannot hold them faithfully, and a span layer can. A Text range is then just one of these strings plus a byte range. Importers and the Starter-library build share one in-memory `ResourceBuilder`, and a single writer turns it into the file.

## Considered Options

- **Container.** Per-Resource SQLite file (chosen); one library-wide database; a custom binary format; a directory of files. A single file per Resource maps one-to-one onto the domain: licensed, imported, shown and removed on its own. SQLite gives us typed tables for datasets, ACID writes at import, and the same engine we use for search and user data. rusqlite now reaches wasm32 through sqlite-wasm-rs. A custom binary format would be pure Rust but makes us own a file format and its tooling.
- **Text model.** Plain string plus standoff spans (chosen); nested markup (XML-like tree); one row per verse with inline markup. Nested markup breaks on overlapping verse and paragraph structure. Rows per verse lose paragraphs and headings that cross verse boundaries, and they don't fit non-Bible Resources.
- **Search index.** A derived cache file per Resource (chosen) versus FTS tables inside the Resource file. Keeping the Resource file immutable means normalizer or tokenizer changes rebuild a cache, not the Resource.

## Consequences

- Resource files are never written after Import. User data (Annotations, Layouts, Preferred order) lives elsewhere.
- Text-range anchors depend on string ids staying stable across Resource revisions. Byte offsets inside a changed string still need a re-anchoring strategy, which is left open (see the Annotation storage ticket).
- Changing the span vocabulary bumps the format version. The engine reads only the versions it knows, and the Starter library is rebuilt each release.
