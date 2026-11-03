//! The Import report: what an Import brought in and what it could not (CONTEXT.md).

use std::collections::BTreeMap;
use std::fmt;

use obs_scripture::Book;

/// One Bible book (or other book-level unit) in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookReport {
    /// The source's book code, e.g. `GEN`, `TOB`, `FRT`.
    pub code: String,
    /// `None` for books outside the 66-book canon, imported as unstructured text.
    pub book: Option<Book>,
    pub chapters: u16,
    pub verses: usize,
}

/// Verse-map figures; filled in once the Verse-map builder runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerseMapCounts {
    pub unmapped: usize,
    pub absent: usize,
}

/// The summary shown after an Import.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub resource_id: String,
    pub title: String,
    /// e.g. `usfm`.
    pub source_format: String,
    pub source_files: Vec<String>,
    pub books: Vec<BookReport>,
    pub verses: usize,
    pub notes: usize,
    pub tagged_words: usize,
    /// Source markup with no place in the span vocabulary, by marker, with counts.
    pub dropped_markup: BTreeMap<String, u64>,
    /// Problems that did not stop the Import (duplicate verses, bad Strong's numbers…).
    pub warnings: Vec<String>,
    /// True when the source carried no licence; the Resource is for personal use only.
    pub license_missing: bool,
    /// `None` until a Verse map has been built for this Bible.
    pub verse_map: Option<VerseMapCounts>,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Imported {} ({})", self.title, self.resource_id)?;
        writeln!(
            f,
            "  from {} file(s), format {}",
            self.source_files.len(),
            self.source_format
        )?;
        let canonical = self.books.iter().filter(|b| b.book.is_some()).count();
        writeln!(
            f,
            "  {canonical} Bible book(s), {} verse(s), {} note(s), {} tagged word(s)",
            self.verses, self.notes, self.tagged_words
        )?;
        let other: Vec<&str> = self
            .books
            .iter()
            .filter(|b| b.book.is_none())
            .map(|b| b.code.as_str())
            .collect();
        if !other.is_empty() {
            writeln!(
                f,
                "  imported as text without verse structure: {}",
                other.join(", ")
            )?;
        }
        match self.verse_map {
            None => writeln!(f, "  verse map: not built yet")?,
            Some(c) => writeln!(
                f,
                "  verse map: {} unmapped verse(s), {} absent verse(s)",
                c.unmapped, c.absent
            )?,
        }
        if self.license_missing {
            writeln!(
                f,
                "  licence: none found in the source; for personal use only"
            )?;
        }
        if !self.dropped_markup.is_empty() {
            let total: u64 = self.dropped_markup.values().sum();
            writeln!(f, "  dropped markup ({total}):")?;
            for (marker, count) in &self.dropped_markup {
                writeln!(f, "    {marker} ×{count}")?;
            }
        }
        if !self.warnings.is_empty() {
            writeln!(f, "  warnings ({}):", self.warnings.len())?;
            for w in self.warnings.iter().take(50) {
                writeln!(f, "    {w}")?;
            }
            if self.warnings.len() > 50 {
                writeln!(f, "    …and {} more", self.warnings.len() - 50)?;
            }
        }
        Ok(())
    }
}
