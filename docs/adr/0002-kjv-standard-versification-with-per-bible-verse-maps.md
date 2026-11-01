# KJV Standard versification with a Verse map built per Bible at import

The Standard versification is KJV numbering of the 66-book canon, plus a title slot for each Psalm superscription. This is the hub that STEPBible's TVTMS maps every tradition onto, and the numbering used by OpenBible.info cross-references, the SWORD default, and every English Bible in the Starter library. Bibles keep and show their own numbering. Each Bible gets its own Verse map when it is imported: the importer runs TVTMS's conditional tests against the verses the Bible actually contains, rather than choosing from a list of named schemes. Every other Passage-keyed Resource (commentaries, Cross-reference sets, people & places) is converted to the Standard versification when it is imported or built, so only Bibles need mapping at runtime.

## Considered Options

- **Standard versification:** KJV-based (chosen). A Hebrew/MT base would force mapping onto every English Bible, Cross-reference set and Annotation. A synthetic superset of all traditions would be ours to design and maintain.
- **Canon:** 66 books only (chosen). KJV + Apocrypha would let Catholic and Orthodox imports sync and take Passage anchors. We accepted that deuterocanonical text, and LXX additions inside canonical books, become Unmapped verses.
- **Verse maps:** per Bible, built from TVTMS tests (chosen). Named schemes, whether picked by the user or auto-detected, fit badly with Bibles that only partly follow a tradition.
- **Where to map:** Bibles native and everything else normalized (chosen). Mapping everything at runtime means more code paths. Normalizing Bibles too would show English numbers in the Hebrew text, and merged or split verses would lose information.

## Consequences

- Refines ADR 0001: Annotations on Unmapped verses fall back to a Text-range anchor in that Bible.
- Verse maps work in whole Verses. TVTMS subverse mappings (`!a`, `.1`) are rounded out to the Verses that contain them.
- TVTMS is CC BY 4.0. The engine embeds it reformatted (allowed), so the app must credit STEP Bible and note any changes we make. We must not ship the raw file as a standalone download.
- Changing the Standard versification later would mean migrating every stored Passage anchor. That makes this decision expensive to reverse.
