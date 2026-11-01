# Bible annotations anchor to Standard-versification Passages

Notes and highlights made in a Bible are anchored to a Passage in the Standard versification, not to a text position in that Bible. A user who highlights John 3:16 in the KJV then sees that highlight in the WEB, the BSB, and the Greek text. Anchors of this kind also survive a Bible being updated or re-imported. Annotations in non-Bible Resources use Text-range anchors instead, because those Resources have no shared Key to fall back on.

## Considered Options

- **Text-range anchors everywhere**: precise to the word, but each highlight appears only in one Bible and breaks when that Bible's text changes.
- **Passage anchor plus an optional Text range from the Bible where it was made** (chosen): verse-level everywhere, and word-level in the Bible where the highlight was created.

## Consequences

- In other Bibles, an annotation shows at whole-Verse granularity.
- Every Bible needs a Verse map to the Standard versification before its annotations can be shown (see #10).
