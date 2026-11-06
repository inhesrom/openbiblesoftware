# The Open catalogue downloads from the publisher, checked by a pin or by the licence notice

Get Resources lists openly licensed Resources that are not SWORD modules from a compiled-in data file, `crates/engine/data/open-catalogue.toml` (#134). Each entry names the publisher's own URL, the importer that reads it, its licence (SPDX and licence class), its Attribution and changes, and archived licence evidence: an evidence file in `crates/engine/data/open-evidence/` and an Internet Archive capture of the publisher's licence page. A download is never re-hosted or mirrored by us. Before it is imported it is checked in one of two ways. If the entry pins a sha256 (the publisher's files are stable), the download must match it. If the entry names a licence file and phrase (the publisher regenerates its files, as eBible.org does with every zip on every build), the licence notice inside the download must still say what the evidence quotes, with no negation or restriction that takes it back: "Not Public Domain", "all rights reserved", a non-commercial or no-derivatives licence however it is written ("CC BY-ND 4.0", "NoDerivs", "non-commercial"), or a denial of sharing ("no redistribution"). A download that fails the check is refused and leaves nothing in the Library. Only Free to share works are listed (Public Domain, CC0, CC BY, CC BY-SA, GPL), and no file comes from ccel.org (#20).

## Considered Options

- **Pin every entry through an Internet Archive capture, as the Starter library does:** the bytes are fixed and can be reviewed, but every download would come from the Internet Archive, not the publisher. The ticket asks for the publisher's own URL, and an archive capture also goes stale when the publisher corrects the text.
- **Download from the publisher, unpinned, with no check:** simple, but if a publisher changed its licence, the app would import the new edition under the old licence and Attribution.
- **Download from the publisher; pin if the files are stable, else check the licence notice in the download (chosen):** users get the publisher's current edition. The licence the app records is checked against the file actually imported, and the archived evidence shows what the licence was when the entry was added.
- **Generate entries from eBible's `translations.csv`:** over a thousand Bibles at once, but the CSV has no licence column (only `Redistributable` and a free-text `Copyright`), so there would be no per-entry evidence.

## Consequences

- Adding an entry is a reviewed data-file change with its evidence file. The `open_catalogue` tests fail if an entry is not Free to share, its class does not match its SPDX id, its evidence is missing or does not cite the archived page, it names a ccel.org URL, or it is checked by neither a pin nor a licence notice.
- An unpinned entry's sha256 is recorded in the Import record (`OpenOrigin::sha256`), so the edition a user has can still be identified afterwards.
- An eBible entry stops importing if eBible removes the licence phrase from `copr.htm`. That is intended: the entry must then be reviewed again.
- Resources downloaded from the Open catalogue are ordinary imported Resources (`Origin::Imported`). Their manifest carries the entry's licence, Attribution and changes, so the About view and Help → Credits (Downloaded open Resources) show them with no extra data.
