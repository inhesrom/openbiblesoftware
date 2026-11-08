//! Hostile-input tests (#253): every reader takes arbitrary bytes, and markup-shaped noise
//! built from its own vocabulary, without panicking. Whatever an import returns is either an
//! error or a Resource whose ranges all slice its text.
//!
//! The inputs come from a seeded generator, so a failure names a seed that replays it.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use obs_model::{Manifest, Resource, ResourceType};
use obs_scripture::Book;

use crate::{
    DatasetOptions, DictionaryData, LexiconOptions, StepbibleImport, StepbibleOptions,
    StepbibleText, StudyImport, UsfmImport, UsfmOptions, import_abbott_smith, import_bdb,
    import_dictionary, import_jfb_book, import_mhc, import_openbible_cross_references,
    import_openbible_geocoding, import_strongs_greek, import_strongs_hebrew, import_tbes,
    import_theographic_events, import_tipnr, usfm_zip_books, zip_text_file,
};

/// Inputs per reader. Small, so the whole suite stays well under a second per reader.
pub(crate) const CASES: u64 = 200;

/// A deterministic xorshift generator: no dependency, and every case replays from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub(crate) fn bytes(&mut self, max: usize) -> Vec<u8> {
        let n = self.below(max + 1);
        (0..n).map(|_| self.next() as u8).collect()
    }

    /// Up to `pieces` items of `vocab`, interleaved with stray bytes and multi-byte characters.
    pub(crate) fn soup(&mut self, vocab: &[&str], pieces: usize) -> String {
        const NOISE: &[&str] = &[
            " ", "\n", "\t", "é", "אָ", "ὁ", "😀", "\u{301}", "&", "&amp;", "&#x3B1;", "&#", ";",
            "<", ">", "\"", "'", "/", "=", "\\", "|", "*", "0", "1", "3:16", "Gen", "John",
        ];
        let mut out = String::new();
        for _ in 0..self.below(pieces + 1) {
            match self.below(10) {
                0 => out.push_str(&String::from_utf8_lossy(&self.bytes(8))),
                1..=3 => out.push_str(NOISE[self.below(NOISE.len())]),
                _ => out.push_str(vocab[self.below(vocab.len())]),
            }
        }
        out
    }

    /// Arbitrary bytes or vocabulary soup, as text.
    pub(crate) fn text(&mut self, vocab: &[&str]) -> String {
        if self.below(4) == 0 {
            String::from_utf8_lossy(&self.bytes(512)).into_owned()
        } else {
            self.soup(vocab, 120)
        }
    }
}

/// Every unit range in a Resource slices that unit's text.
pub(crate) fn assert_ranges_slice(resource: &Resource, seed: u64) {
    resource
        .validate()
        .unwrap_or_else(|e| panic!("seed {seed}: imported Resource is invalid: {e}"));
}

fn dataset() -> DatasetOptions {
    DatasetOptions {
        resource_id: "hostile".into(),
        title: "Hostile".into(),
        abbreviation: "H".into(),
        ..DatasetOptions::default()
    }
}

fn lexicon() -> LexiconOptions {
    LexiconOptions {
        resource_id: "hostile".into(),
        title: "Hostile".into(),
        abbreviation: "H".into(),
        ..LexiconOptions::default()
    }
}

const USFM: &[&str] = &[
    "\\id GEN ",
    "\\id JHN ",
    "\\h ",
    "\\mt1 ",
    "\\c 1",
    "\\c 3",
    "\\c ",
    "\\v 1 ",
    "\\v 16 ",
    "\\v 2-3 ",
    "\\v ",
    "\\p ",
    "\\q1 ",
    "\\q2 ",
    "\\s1 ",
    "\\d ",
    "\\f + ",
    "\\fr 1:1 ",
    "\\ft ",
    "\\f*",
    "\\x - ",
    "\\xo ",
    "\\xt ",
    "\\x*",
    "\\w ",
    "|strong=\"H7225\"",
    "\\w*",
    "\\wj ",
    "\\wj*",
    "\\add ",
    "\\add*",
    "\\nd ",
    "\\nd*",
    "\\it ",
    "\\it*",
    "\\+w ",
    "\\+w*",
    "\\b",
    "\\li1 ",
    "\\m ",
    "\\ip ",
    "\\rem ",
    "\\zz ",
    "\\",
    "In the beginning ",
    "God",
];

const XML: &[&str] = &[
    "<note>",
    "</note>",
    "<note type=\"crossReference\">",
    "<note sID=\"n\"/>",
    "<note eID=\"n\"/>",
    "<reference>",
    "</reference>",
    "<reference osisRef=\"John.3.16\">",
    "<reference sID=\"r\" osisRef=\"Gen.1.1\"/>",
    "<reference eID=\"r\"/>",
    "<q who=\"Jesus\" sID=\"q\"/>",
    "<q eID=\"q\"/>",
    "<q who=\"Jesus\">",
    "</q>",
    "<p>",
    "</p>",
    "<l>",
    "</l>",
    "<l sID=\"l\"/>",
    "<l eID=\"l\"/>",
    "<lg>",
    "</lg>",
    "<hi type=\"italic\">",
    "</hi>",
    "<title>",
    "</title>",
    "<title type=\"psalm\">",
    "<w lemma=\"strong:G25\">",
    "<w lemma=\"strong:H7225\" morph=\"x\">",
    "</w>",
    "<transChange>",
    "</transChange>",
    "<divineName>",
    "</divineName>",
    "<verse osisID=\"John.3.16\" sID=\"v\"/>",
    "<verse eID=\"v\"/>",
    "<chapter osisID=\"John.3\"/>",
    "<div type=\"paragraph\"/>",
    "<milestone type=\"x-p\"/>",
    "<lb/>",
    "<figure/>",
    "<index/>",
    "<scripRef>",
    "</scripRef>",
    "<scripRef passage=\"Gen 1:1\">",
    "<sync type=\"Strongs\" value=\"G25\"/>",
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<br/>",
    "<FR>",
    "<Fr>",
    "<FI>",
    "<Fi>",
    "<RF>",
    "<Rf>",
    "<WG25>",
    "<WH7225>",
    "<WTG5719>",
    "<CM>",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "<?x?>",
    "<entry strongs=\"25\">",
    "</entry>",
    "<div type=\"entry\" n=\"H1\">",
    "</div>",
    "<entry key=\"H1\">",
    "<sense>",
    "</sense>",
    "<orth>",
    "</orth>",
    "<def>",
    "</def>",
    "<ref osisRef=\"Gen.1.1\">",
    "</ref>",
    "<foreign>",
    "</foreign>",
    "In the beginning",
];

const TABLE: &[&str] = &[
    "\t",
    "\n",
    "\r\n",
    "Gen.1.1",
    "John.3.16",
    "Gen.1.1-Gen.1.3",
    "Gen 1:1",
    "Jhn.3.16",
    "H7225",
    "G25",
    "H0001G",
    "=",
    "$",
    "@",
    "#",
    "–",
    "From Verse",
    "To Verse",
    "Votes",
    "12",
    "-3",
    "99999999999999999999",
    "Aaron",
    "Abraham@Gen.11.26-Gen.25.10",
    "Place",
    "Person",
    "\"",
    ",",
    "[",
    "]",
    "{",
    "}",
    ":",
    "null",
    "true",
    "Gen.1.1+Gen.1.3",
];

#[test]
fn usfm_import_never_panics_on_hostile_input() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut import = UsfmImport::new(UsfmOptions {
            resource_id: "hostile".into(),
            ..UsfmOptions::default()
        });
        for i in 0..1 + rng.below(3) {
            let _ = import.add_book(&format!("{i}.usfm"), rng.text(USFM));
        }
        if let Ok((resource, _)) = import.finish() {
            assert_ranges_slice(&resource, seed);
        }
    }
}

#[test]
fn zip_readers_never_panic_on_hostile_input() {
    let good = {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in [("GEN.usfm", "\\id GEN\n\\c 1\n\\v 1 In"), ("LICENSE", "x")] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    };
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let bytes = if rng.below(2) == 0 {
            rng.bytes(2048)
        } else {
            // A real zip with a few bytes changed: past the signature checks, into the entries.
            let mut bytes = good.clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(bytes.len());
                bytes[at] = rng.next() as u8;
            }
            bytes
        };
        let _ = usfm_zip_books(&bytes);
        let _ = zip_text_file(&bytes, "LICENSE");
    }
}

#[test]
fn stepbible_import_never_panics_on_hostile_input() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let text = if seed % 2 == 0 {
            StepbibleText::Tahot
        } else {
            StepbibleText::Tagnt
        };
        let mut import = StepbibleImport::new(StepbibleOptions {
            text,
            resource_id: "hostile".into(),
            ..StepbibleOptions::default()
        });
        import.add_file("hostile.txt", rng.text(TABLE));
        if let Ok((resource, _)) = import.finish() {
            assert_ranges_slice(&resource, seed);
        }
    }
}

#[test]
fn dataset_imports_never_panic_on_hostile_input() {
    let o = dataset();
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let results = [
            import_openbible_cross_references(&rng.text(TABLE), &o).map(|r| r.0),
            import_tipnr(&rng.text(TABLE), &o).map(|r| r.0),
            import_openbible_geocoding(&rng.text(TABLE), &rng.text(TABLE), None, &o).map(|r| r.0),
            import_theographic_events(
                &rng.text(TABLE),
                &rng.text(TABLE),
                &rng.text(TABLE),
                None,
                &o,
            )
            .map(|r| r.0),
        ];
        for resource in results.into_iter().flatten() {
            assert_ranges_slice(&resource, seed);
        }
    }
}

#[test]
fn lexicon_imports_never_panic_on_hostile_input() {
    let o = lexicon();
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let results = [
            import_strongs_greek(&o, &rng.text(XML)).map(|r| r.0),
            import_strongs_hebrew(&o, &rng.text(XML)).map(|r| r.0),
            import_abbott_smith(&o, &rng.text(XML)).map(|r| r.0),
            import_bdb(&o, &rng.text(XML), &rng.text(XML)).map(|r| r.0),
            import_tbes(&o, &rng.text(TABLE), &[]).map(|r| r.0),
        ];
        for resource in results.into_iter().flatten() {
            assert_ranges_slice(&resource, seed);
        }
    }
}

#[test]
fn commentary_and_dictionary_imports_never_panic_on_hostile_input() {
    let study = |kind| StudyImport::new(Manifest::new("hostile", "Hostile", "H", kind, "en"));
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut mhc = study(ResourceType::Commentary);
        let _ = import_mhc(&mut mhc, &rng.text(XML));
        let mut jfb = study(ResourceType::Commentary);
        let _ = import_jfb_book(&mut jfb, Book::John, &rng.text(XML));
        let (index, data, block_index, blocks) =
            (rng.bytes(64), rng.bytes(256), rng.bytes(64), rng.bytes(256));
        let mut easton = study(ResourceType::Dictionary);
        let _ = import_dictionary(
            &mut easton,
            DictionaryData::Easton {
                index: &index,
                data: &data,
                block_index: &block_index,
                blocks: &blocks,
            },
        );
        let mut smith = study(ResourceType::Dictionary);
        let _ = import_dictionary(
            &mut smith,
            DictionaryData::Smith {
                index: &index,
                data: &data,
            },
        );
        for import in [mhc, jfb, easton, smith] {
            if let Ok((resource, _)) = import.finish() {
                assert_ranges_slice(&resource, seed);
            }
        }
    }
}
