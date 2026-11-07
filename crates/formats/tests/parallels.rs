//! A USFM Bible's parallel passage references (`\r`) → Parallel passages (#212), against an
//! excerpt of the Berean Standard Bible (public domain): Mark 1:1-15, Matthew 3, Luke 3:1-22,
//! John 1:1-34, Isaiah 40:1-5, 2 Samuel 24 and 1 Chronicles 21.
//!
//! The whole BSB is checked too when `OBS_TEST_BSB_USFM` points at `bsb_usfm.zip`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use obs_formats::{
    DatasetOptions, USFM_PARALLELS_FORMAT, import_usfm_parallel_passages, usfm_zip_books,
};
use obs_model::{ParallelSection, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};

fn options() -> DatasetOptions {
    DatasetOptions {
        resource_id: "bsb-parallels".into(),
        title: "BSB Parallel Passages".into(),
        source_file: "bsb_usfm.zip".into(),
        tool_version: "test".into(),
        ..DatasetOptions::default()
    }
}

fn excerpt() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/bsb-parallels");
    let mut books: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(path).unwrap())
        })
        .collect();
    books.sort();
    books
}

fn passage(book: Book, chapter: u16, start: u16, end: u16) -> Passage {
    Passage::new(
        Verse::new(book, chapter, start),
        Verse::new(book, chapter, end),
    )
    .unwrap()
}

fn section(sections: &[ParallelSection], at: Passage) -> &ParallelSection {
    sections
        .iter()
        .find(|s| s.passage == at)
        .unwrap_or_else(|| panic!("no section at {at}"))
}

/// Each heading with a `\r` line becomes a section from its first Verse to the Verse before
/// the next heading, titled by the heading, with its references in the order written.
#[test]
fn bsb_sections_list_their_parallels_in_the_order_written() {
    let (r, report) = import_usfm_parallel_passages(&excerpt(), &options()).unwrap();
    assert_eq!(r.manifest.resource_type, ResourceType::CrossReferenceSet);
    assert_eq!(r.manifest.source_format, USFM_PARALLELS_FORMAT);
    assert!(r.cross_references.is_empty());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.parallel_sections, r.parallel_sections.len());
    assert!(
        report
            .to_string()
            .contains("section(s) with parallel passages")
    );

    let mark = section(&r.parallel_sections, passage(Book::Mark, 1, 1, 11));
    assert_eq!(mark.title, "The Mission of John the Baptist");
    assert_eq!(
        mark.parallels,
        [
            passage(Book::Isaiah, 40, 1, 5),
            passage(Book::Matthew, 3, 1, 17),
            passage(Book::Luke, 3, 1, 22),
            passage(Book::John, 1, 19, 34),
        ]
    );
    let baptism = section(&r.parallel_sections, passage(Book::Matthew, 3, 13, 17));
    assert_eq!(baptism.title, "The Baptism of Jesus");
    assert_eq!(
        baptism.parallels,
        [
            passage(Book::Mark, 1, 9, 11),
            passage(Book::Luke, 3, 21, 22),
            passage(Book::John, 1, 29, 34),
        ]
    );
    let census = section(&r.parallel_sections, passage(Book::SecondSamuel, 24, 1, 9));
    assert_eq!(census.title, "David’s Military Census");
    assert_eq!(
        census.parallels,
        [
            passage(Book::Exodus, 30, 11, 16),
            passage(Book::FirstChronicles, 21, 1, 6),
        ]
    );
    let altar = section(
        &r.parallel_sections,
        passage(Book::FirstChronicles, 21, 18, 30),
    );
    assert_eq!(altar.parallels, [passage(Book::SecondSamuel, 24, 18, 25)]);
}

/// The whole BSB, when `OBS_TEST_BSB_USFM` names the pinned `bsb_usfm.zip`: every `\r` section
/// is kept with Standard Passages, and only the references the converter documents are dropped.
#[test]
fn whole_bsb_when_available() {
    let Ok(path) = std::env::var("OBS_TEST_BSB_USFM") else {
        return;
    };
    let books = usfm_zip_books(&std::fs::read(path).unwrap()).unwrap();
    let (r, report) = import_usfm_parallel_passages(&books, &options()).unwrap();
    let parallels: usize = r.parallel_sections.iter().map(|s| s.parallels.len()).sum();
    println!(
        "{} sections, {parallels} parallels, dropped {:?}\n{}",
        r.parallel_sections.len(),
        r.dropped_markup,
        report.warnings.join("\n")
    );
    assert!(
        r.parallel_sections.len() > 1300,
        "{}",
        r.parallel_sections.len()
    );
    for s in &r.parallel_sections {
        for p in std::iter::once(&s.passage).chain(&s.parallels) {
            assert!(Standard.contains(p.start()) && Standard.contains(p.end()));
        }
    }
    assert!(report.warnings.len() < 10, "{:?}", report.warnings);
}
