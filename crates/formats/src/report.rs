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
    /// Cross-references kept (Cross-reference set).
    pub cross_references: usize,
    /// People, groups and places kept (People & places index).
    pub entities: usize,
    /// Mentions kept (People & places index), one per entity and Verse.
    pub mentions: usize,
    /// Source markup with no place in the span vocabulary, by marker, with counts.
    pub dropped_markup: BTreeMap<String, u64>,
    /// Problems that did not stop the Import (duplicate verses, bad Strong's numbers…).
    pub warnings: Vec<String>,
    /// True when the source carried no licence; the Resource is for personal use only.
    pub license_missing: bool,
    /// `None` until a Verse map has been built for this Bible.
    pub verse_map: Option<VerseMapCounts>,
    /// Places where the Verse map followed a numbering tradition other than the Standard's,
    /// by TVTMS tradition name.
    pub traditions: BTreeMap<String, usize>,
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
        let dataset = self.cross_references + self.entities + self.mentions > 0;
        if self.cross_references > 0 {
            writeln!(f, "  {} cross-reference(s)", self.cross_references)?;
        }
        if self.entities + self.mentions > 0 {
            writeln!(
                f,
                "  {} person(s) or place(s), {} mention(s)",
                self.entities, self.mentions
            )?;
        }
        if !dataset {
            let canonical = self.books.iter().filter(|b| b.book.is_some()).count();
            writeln!(
                f,
                "  {canonical} Bible book(s), {} verse(s), {} note(s), {} tagged word(s)",
                self.verses, self.notes, self.tagged_words
            )?;
        }
        let outside: Vec<&str> = self
            .books
            .iter()
            .filter(|b| b.book.is_none())
            .map(|b| b.code.as_str())
            .collect();
        if !outside.is_empty() {
            writeln!(
                f,
                "  outside the 66-book canon, imported as text (Unmapped verses): {}",
                outside.join(", ")
            )?;
        }
        match self.verse_map {
            None if dataset => {}
            None => writeln!(f, "  verse map: not built yet")?,
            Some(c) => writeln!(
                f,
                "  verse map: {} unmapped verse(s), {} absent verse(s)",
                c.unmapped, c.absent
            )?,
        }
        let other: Vec<String> = self
            .traditions
            .iter()
            .filter(|(t, _)| {
                !t.starts_with("Eng-KJV") && *t != "AllBibles" && *t != "EngTitleSeparate"
            })
            .map(|(t, n)| format!("{t} ×{n}"))
            .collect();
        if !other.is_empty() {
            writeln!(f, "  numbering differs from the KJV: {}", other.join(", "))?;
        }
        if self.license_missing {
            writeln!(
                f,
                "  licence: none found in the source; for personal use only"
            )?;
        }
        if !self.dropped_markup.is_empty() {
            let total: u64 = self.dropped_markup.values().sum();
            let what = if dataset { "dropped" } else { "dropped markup" };
            writeln!(f, "  {what} ({total}):")?;
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
