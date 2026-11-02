# TVTMS (embedded engine data)

`tvtms-canon.tsv` is a reformatted extract of **TVTMS** (Translators Versification Traditions
with Methodology for Standardisation) by **STEP Bible**, <https://www.STEPBible.org>, from
[STEPBible-Data](https://github.com/STEPBible/STEPBible-Data) (`Versification/`), licensed
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

The engine embeds it to build each Bible's Verse map at Import (ADR 0002). It is credited in
Help → Credits.

**Changes we made:** kept only rows of the expanded data section whose SourceRef is in a book
of the 66-book canon; kept only the columns SourceType, SourceRef, StandardRef, Action and
Tests; trimmed whitespace. No mapping or test was edited.

Regenerate from a new TVTMS release with `cargo xtask tvtms <path to the TVTMS .txt>` and
review the diff. Per STEP Bible's terms, do not distribute this file on its own; it ships only
inside the app.
