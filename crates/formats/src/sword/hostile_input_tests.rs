//! Hostile-input tests for the SWORD markup parsers (#253): OSIS, ThML and GBF noise, fed
//! across verses and units like a real module, never panics, and every range the [`Writer`]
//! hands over slices the text it belongs to.

#![allow(clippy::unwrap_used)]

use std::ops::Range;

use obs_scripture::{Book, Verse};

use super::writer::Writer;
use super::{gbf, osis, thml};
use crate::hostile_input::{CASES, Rng};

const OSIS: &[&str] = &[
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
    "<reference sID=\"s\"/>",
    "<reference eID=\"s\"/>",
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
    "<hi type=\"bold\" sID=\"h\"/>",
    "<hi eID=\"h\"/>",
    "</hi>",
    "<title>",
    "</title>",
    "<title type=\"psalm\">",
    "<title type=\"psalm\" subType=\"x-preverse\">",
    "<title type=\"x-gen\">",
    "<w lemma=\"strong:G25\">",
    "<w lemma=\"strong:H7225\" morph=\"x\">",
    "</w>",
    "<transChange>",
    "</transChange>",
    "<divineName>",
    "</divineName>",
    "<foreign>",
    "</foreign>",
    "<div type=\"paragraph\"/>",
    "<div type=\"x-p\" sID=\"d\"/>",
    "<div eID=\"d\"/>",
    "<milestone type=\"x-p\"/>",
    "<lb/>",
    "<figure>",
    "</figure>",
    "<index/>",
    "<item>",
    "</item>",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "v. 5",
    "John 3:16",
    "In the beginning",
    "God",
];

const THML: &[&str] = &[
    "<scripRef>",
    "</scripRef>",
    "<scripRef passage=\"Gen 1:1\">",
    "<note>",
    "</note>",
    "<sync type=\"Strongs\" value=\"G25\"/>",
    "<sync type=\"morph\" value=\"V-PAI\"/>",
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<p>",
    "</p>",
    "<br/>",
    "<h3>",
    "</h3>",
    "<font color=\"red\">",
    "</font>",
    "<div class=\"sechead\">",
    "</div>",
    "<a href=\"x\">",
    "</a>",
    "<img/>",
    "<table>",
    "</table>",
    "<foreign lang=\"he\">",
    "</foreign>",
    "John 3:16",
    "v. 5",
];

const GBF: &[&str] = &[
    "<FR>",
    "<Fr>",
    "<FI>",
    "<Fi>",
    "<FB>",
    "<Fb>",
    "<FU>",
    "<Fu>",
    "<TS>",
    "<Ts>",
    "<TT>",
    "<Tt>",
    "<TH>",
    "<Th>",
    "<RF>",
    "<Rf>",
    "<RX>",
    "<Rx>",
    "<WG25>",
    "<WH430>",
    "<WTG5719>",
    "<CM>",
    "<CL>",
    "<ZZ>",
    "<",
    ">",
    "John 3:16",
    "word",
];

fn check_ranges(what: &str, text: &str, ranges: impl IntoIterator<Item = Range<usize>>) {
    for range in ranges {
        assert!(
            text.get(range.clone()).is_some(),
            "{what}: {range:?} does not slice {text:?}"
        );
    }
}

/// Feed `entries` like an import does (a book of verses across chapters, then the next
/// unit) and check every range handed over.
fn run(feed: fn(&str, &mut Writer), vocab: &[&str]) {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut w = Writer::new();
        for _ in 0..1 + rng.below(2) {
            let book = [Book::Psalms, Book::John][rng.below(2)];
            w.set_book(Some(book));
            for _ in 0..1 + rng.below(4) {
                let chapter = 1 + rng.below(3) as u16;
                match rng.below(4) {
                    0 => w.start_chapter(chapter),
                    1 => w.start_intro(),
                    _ => {
                        let verse = Verse::new(book, chapter, rng.below(4) as u16);
                        w.open_verse(verse, verse.verse);
                    }
                }
                feed(&rng.soup(vocab, 40), &mut w);
            }
            let unit = w.finish_unit();
            let what = format!("seed {seed}");
            check_ranges(&what, &unit.text, unit.spans.iter().map(|(r, _)| r.clone()));
            check_ranges(
                &what,
                &unit.text,
                unit.verses.iter().map(|(r, _, _)| r.clone()),
            );
            for (id, note) in w.take_notes() {
                let what = format!("seed {seed}, {id}");
                check_ranges(&what, &note.text, note.spans.iter().map(|(r, _)| r.clone()));
            }
        }
    }
}

#[test]
fn osis_never_panics_and_keeps_ranges_in_their_text() {
    run(osis::feed, OSIS);
}

#[test]
fn thml_never_panics_and_keeps_ranges_in_their_text() {
    run(thml::feed, THML);
}

#[test]
fn gbf_never_panics_and_keeps_ranges_in_their_text() {
    run(gbf::feed, GBF);
}
