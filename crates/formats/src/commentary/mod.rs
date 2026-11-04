//! Starter Commentary and Dictionary edition converters. Passage Keys and reference spans
//! use the Standard versification; these editions follow KJV numbering (ADR 0002).

mod dictionary;
mod html;
mod jfb;
mod mhc;
mod references;

use anyhow::{Result, bail};
use obs_model::{
    Key, KeyScheme, Manifest, Resource, ResourceBuilder, ResourceType, UnitId, UnitKind,
};
use obs_scripture::{Passage, Standard};
use std::collections::{BTreeMap, BTreeSet};

use crate::{ImportReport, text_buf::TextBuf};
pub use dictionary::{DictionaryData, import_dictionary};
pub use jfb::import_jfb_book;
pub use mhc::import_mhc;

/// Collects edition sections before writing through the shared ResourceBuilder.
/// Archive and download handling belong to the caller, not these pure-data converters.
pub struct StudyImport {
    builder: ResourceBuilder,
    report: ImportReport,
    sections: Vec<(Key, String, TextBuf)>,
    aliases: Vec<(String, String)>,
}

impl StudyImport {
    pub fn new(mut manifest: Manifest) -> Self {
        manifest.key_schemes = vec![if manifest.resource_type == ResourceType::Dictionary {
            KeyScheme::Headword
        } else {
            KeyScheme::Passage
        }];
        let report = ImportReport {
            resource_id: manifest.resource_id.clone(),
            title: manifest.title.clone(),
            source_format: manifest.source_format.clone(),
            source_files: manifest.source_file.iter().cloned().collect(),
            license_missing: manifest.license == obs_model::LICENSE_UNKNOWN,
            ..ImportReport::default()
        };
        Self {
            builder: ResourceBuilder::new(manifest),
            report,
            sections: Vec::new(),
            aliases: Vec::new(),
        }
    }

    fn section(&mut self, passage: Passage, suffix: &str, text: TextBuf) {
        let id = format!(
            "{}-{}{}",
            passage.start().osis(),
            passage.end().osis(),
            suffix
        );
        if !text.text.trim().is_empty() {
            self.sections.push((Key::Passage(passage), id, text));
        }
    }

    /// Validate all spans and Keys and compute the normal Resource content hash.
    pub fn finish(mut self) -> Result<(Resource, ImportReport)> {
        if self.sections.is_empty() {
            bail!("no commentary sections or dictionary entries found");
        }
        self.sections
            .sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        let kind = if self.builder.manifest().resource_type == ResourceType::Dictionary {
            UnitKind::Entry
        } else {
            UnitKind::Section
        };
        let mut coverage: BTreeMap<
            obs_scripture::Book,
            (BTreeSet<u16>, BTreeSet<obs_scripture::Verse>),
        > = BTreeMap::new();
        for (key, id, text) in self.sections {
            if let Key::Passage(p) = &key
                && !id.ends_with(".book-intro")
            {
                let (chapters, verses) = coverage.entry(p.start().book).or_default();
                chapters.insert(p.start().chapter);
                if !id.ends_with(".chapter-intro") {
                    verses.extend(p.verses(&Standard).filter(|v| v.verse > 0));
                }
            }
            let unit = UnitId::new(id)?;
            self.builder.add_key(key.clone(), &unit, None, false);
            for (range, span) in text.spans {
                self.builder.add_span(&unit, range, span);
            }
            self.builder
                .add_unit(unit, kind, Some(key.to_string()), text.text);
        }
        self.report.books = coverage
            .into_iter()
            .map(|(book, (chapters, verses))| crate::BookReport {
                code: book.usfm().into(),
                book: Some(book),
                chapters: chapters.len() as u16,
                verses: verses.len(),
            })
            .collect();
        self.report.verses = self.report.books.iter().map(|b| b.verses).sum();
        for (alias, id) in self.aliases {
            self.builder
                .add_key(Key::Headword(alias), &UnitId::new(id)?, None, true);
        }
        for (name, count) in &self.report.dropped_markup {
            for _ in 0..*count {
                self.builder.count_dropped(name);
            }
        }
        Ok((self.builder.build()?, self.report))
    }
}

#[cfg(test)]
mod tests;
