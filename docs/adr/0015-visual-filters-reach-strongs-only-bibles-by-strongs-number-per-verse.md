# Visual filters reach a Strong's-only Bible by Strong's number, Verse by Verse

A Visual filter (#211) is a Search query painted over the text being read. Its Morphology and
Lemma terms (`@imperative`, `lemma:ἀγαπάω`) need data only an original-language Bible carries:
the Starter KJV is KJV+Strong's, whose Tagged words have a Strong's number and no Morphology or
Lemma. The ticket asks that "imperatives in Romans 12" colour the right words in TAGNT **and**
in the KJV. We decided:

1. **Strong's alignment is by number within one Standard Verse.** For each Verse of the Bible
   being read, take the same Standard Verse in the first Hebrew (Old Testament) or Greek (New
   Testament) Bible in Preferred order that has it (the Exegetical Guide's rule); find the words
   there that the query matches; colour the words of the Verse being read that carry one of
   those words' Strong's numbers (base forms, so KJV `G4160` matches TAGNT `G4160G`). Romans 12:14
   colours KJV "Bless … bless … curse" because TAGNT's εὐλογεῖτε and καταρᾶσθε are imperatives,
   and leaves "persecute" (G1377, a participle in TAGNT) plain.
2. **Grammatical morphemes are never aligned.** STEPBible numbers prefixes and suffixes from
   H9001 up; every Hebrew word has one (`וְ`), so aligning them would colour every "and".
3. **Filters are a preference, not a table.** The list is the `reading.visual_filters` User
   database preference (JSON), as Collections are; which filters a Tab switched is kept in the
   Tab (Layouts and the Last session), as Interlinear lines are. No schema change.

## Considered Options

- **Word-level alignment data (a translation's word ↔ original word table):** would also tell
  apart two uses of one number in a Verse, but no open alignment of the KJV to TAGNT/TAHOT is
  in the library, and building one is a dataset project of its own.
- **Colour every word with the number anywhere in the chapter or book:** simpler, but "bless"
  in a Verse where εὐλογέω is a participle would be painted as an imperative.
- **Only original-language Bibles get Morphology filters:** honest but fails the ticket's
  criterion and the most common reading case (an English Bible).
- **A `visual_filters` table in the User database:** typed columns, but every change would need a
  migration and a backup format note; the list is small and read whole.

## Consequences

- A Verse where one Strong's number occurs both as a match and not (two forms of one verb) colours
  every English word with that number: alignment is per Verse, not per word.
- Without a Hebrew or Greek Bible in the library, Morphology and Lemma filters colour nothing in
  a Strong's-only Bible; Strong's-number and word filters still do.
- Matching runs per visible block on a worker; the original-language Bible is read there the
  first time a block needs it.
