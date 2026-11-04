# Study Resource golden fixtures (#44)

Public-domain excerpts from the exact locked editions in `starter/sources.toml`.
Licence evidence: `starter/evidence/{mhc,jfb,easton,smith}.md` and the archived notices
referenced there. MHC and Easton credit their CCEL transcription lineage; these bytes
were downloaded only from Wikisource/Internet Archive, thirdmill/Internet Archive,
and CrossWire, never from CCEL.

- `mhc.xml`: Genesis chapter 1 and Luke chapter 18 pages; XML wrapper reduced to title/text.
- `jfb-genesis-1.htm`: JFB01.htm through the start of chapter 2, decoded from Latin-1 to UTF-8.
- `easton.*`: Abraham and Abram index records, offsets rebased; original zlib block unchanged.
- `smith.*`: Ab (both senses), Abraham and Abram, index offsets rebased; original Windows-1252 text unchanged.
- `golden-*.json`: converter output of one section/entry each (plain NFC text and standoff spans).

Full source hashes and notices remain in the real lock and evidence files. These small
fixtures need no network; the ignored xtask test checks full pinned downloads separately.
