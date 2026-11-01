# Anchors store the quoted text and its context; Annotations are never silently dropped

A Text range is stored as (Resource, text unit id, byte start, byte end). Each one also stores the **quoted text** and about 32 characters of **context on each side**, plus the content hash of the Resource revision it was made against. When that Resource gets a new revision, or is re-imported under the same id, the engine re-anchors. It keeps the range if the quote still matches at the stored offsets. Otherwise it looks for the quote in the same text unit and picks the occurrence whose context and position fit best, first exactly and then ignoring whitespace and punctuation. If none is found, the Annotation becomes an **Orphaned annotation**. It is kept with its quote and listed for the user to re-attach or detach. Nothing is ever deleted automatically. A Passage anchor's Standard Passage never needs re-anchoring (ADR 0001/0002); only its optional Text range goes through these steps, and if it fails the Highlight falls back to whole Verses.

## Considered Options

- **Offsets only:** smallest, but every typo-fix revision silently moves Highlights onto the wrong words.
- **Quote + context + offsets (chosen):** a few dozen extra bytes per Anchor. The same approach as web-annotation selectors, and it handles the common case (small edits elsewhere in the unit) without user effort.
- **Fuzzy (approximate-string) matching** of edited quotes: catches more cases but risks silent wrong matches. Left out of v0.1; orphaning is the safe failure.

## Consequences

- The quote and context must be captured when an Annotation is created, from v0.1 on. Without them, Annotations made earlier could not be re-anchored later. That is the irreversible part of this decision.
- Removing a Resource keeps its Annotations dormant. Re-importing a Resource with the same id brings them back through the same re-anchoring steps.
