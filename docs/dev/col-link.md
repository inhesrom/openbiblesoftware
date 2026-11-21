# Search Christian Open Library (#343)

A link out to the Christian Open Library's website (christianopenlibrary.org), no download:
`col.search_website {text}` opens `https://christianopenlibrary.org/#q=…`
(`obs_engine::col_link::website_search_url`: whitespace collapsed, cut at a word to the site's
500 characters, percent-encoded) in the system browser through `Effect::OpenUrl`. The search is
after the `#`, which the browser keeps to itself: COL's page reads it, and its server never
receives it. The app's transport is not used (tested with a recording transport).

- The right-click menu offers it for a Selection (the selected text) and a Verse (its reference
  in the Standard versification).
- The Passage Guide's Cited in section ends with "Search the Christian Open Library for
  <Passage>".
- Settings → Search → **Search Christian Open Library** (`col.search_link {on}`, preference
  `col/search-link`, on by default) hides every entry point. A Backup's trial restore compares
  it; a restore reads it again.

COL's reuse terms allow linking to its website. Downloading its collection into the app needs
COL's written licence (more than 10 books), so the app's own collection of these works comes
from the original publishers instead (#345).
