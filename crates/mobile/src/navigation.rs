//! Going places in a reader: typed references and Keys, the book, chapter and verse grid,
//! Back and Forward, and History (CONTEXT.md: Visit). Every jump is a Workspace command or
//! call on the reader's Tab, so the Tab's history and History's Visits are the desktop's.

use obs_engine::TabKind;
use obs_model::{Key, ResourceType};
use obs_scripture::{
    Book, RefStyle, Standard, Verse, Versification, format_passage, parse_reference,
};
use serde_json::json;

use crate::readers::ReaderSnapshot;
use crate::{MobileApp, MobileError, ReaderId};

/// A typed reference as the reader reads it, before going there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReference {
    /// How the bar names it: `John 3:16`, `1 Corinthians 13`, or the entry's Key.
    pub label: String,
    /// The first Verse's OSIS id (`John.3.16`), or the Key as written for an entry.
    pub id: String,
}

/// A Bible book a reader's Resource has, for the book and chapter grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookChapters {
    /// The book's OSIS id (`Gen`, `1Cor`): what [`MobileApp::chapters`] takes.
    pub id: String,
    pub name: String,
    /// How many chapters it has there.
    pub chapters: u16,
}

/// One day of History, newest Visit first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDay {
    /// "Today", "Yesterday", or the date.
    pub heading: String,
    pub visits: Vec<HistoryVisit>,
}

/// One Visit in History.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryVisit {
    pub id: i64,
    /// "Romans 8:28 · BSB".
    pub label: String,
    /// The local time of day, `HH:MM`.
    pub time: String,
}

impl MobileApp {
    /// Go to `text` in reader `reader`: a Bible reference read in its Resource's own
    /// numbering with its Current location as context (`Jn 3:16`, `3:16`, `1 Co 13`), or for
    /// a Lexicon or Dictionary an entry Key (a Strong's number, Lemma or headword). A jump:
    /// a step in its Back/Forward history and a Visit in History. The error is the parser's.
    pub fn navigate(
        &mut self,
        reader: ReaderId,
        text: &str,
    ) -> Result<ReaderSnapshot, MobileError> {
        let tab = self.reader_at(reader)?.tab;
        (self
            .session
            .run("tab.go", &json!({ "tab": tab.0, "text": text })))
        .map_err(|e| MobileError::Reference {
            reference: text.to_string(),
            error: crate::app::failed(e).to_string(),
        })?;
        self.apply_effects();
        self.reader(reader)
    }

    /// Read `text` as reader `reader` would go to it ([`Self::navigate`]), without going.
    pub fn parse_reference(
        &self,
        reader: ReaderId,
        text: &str,
    ) -> Result<ParsedReference, MobileError> {
        let r = self.reader_at(reader)?;
        let refused = |error: String| MobileError::Reference {
            reference: text.to_string(),
            error,
        };
        let tab = self.session.workspace.tab(r.tab);
        if tab.is_some_and(obs_engine::Tab::keeps_entries) {
            let entry = (r.view.doc.navigation.parse_entry(text))
                .ok_or_else(|| refused(format!("No entry for {text}")))?;
            return Ok(ParsedReference {
                label: entry.key.to_string(),
                id: entry.key.to_string(),
            });
        }
        let context = tab.and_then(|t| t.history.current());
        let bible = r.view.doc.manifest.resource_type == ResourceType::Bible;
        let own = &r.view.doc.versification;
        let list = if bible {
            parse_reference(text, own, context)
        } else {
            parse_reference(text, &Standard, context)
        }
        .map_err(|e| refused(e.kind.to_string()))?;
        let first =
            (list.passages().first().copied()).ok_or_else(|| refused("no reference".into()))?;
        if bible && !own.contains(first.start()) {
            return Err(refused(format!("{} is not in this Bible", first.start())));
        }
        let label = if bible {
            format_passage(first, own, RefStyle::Long)
        } else {
            format_passage(first, &Standard, RefStyle::Long)
        };
        Ok(ParsedReference {
            label,
            id: first.start().osis(),
        })
    }

    /// Back one step in reader `reader`'s history (nothing happens at its start).
    pub fn back(&mut self, reader: ReaderId) -> Result<ReaderSnapshot, MobileError> {
        self.step(reader, "tab.back")
    }

    /// Forward one step in reader `reader`'s history (nothing happens at its end).
    pub fn forward(&mut self, reader: ReaderId) -> Result<ReaderSnapshot, MobileError> {
        self.step(reader, "tab.forward")
    }

    fn step(&mut self, reader: ReaderId, command: &str) -> Result<ReaderSnapshot, MobileError> {
        let tab = self.reader_at(reader)?.tab;
        (self.session.run(command, &json!({ "tab": tab.0 }))).map_err(crate::app::failed)?;
        self.apply_effects();
        self.reader(reader)
    }

    /// Where the grid's rows come from: each Verse of a Bible, or the first Verse of each
    /// section of a Commentary, in the Resource's order.
    fn grid_verses(&self, reader: ReaderId) -> Result<Vec<Verse>, MobileError> {
        let doc = &self.reader_at(reader)?.view.doc;
        if doc.manifest.resource_type == ResourceType::Bible {
            return Ok(doc.verses.iter().map(|v| v.verse).collect());
        }
        Ok((doc.navigation.entries())
            .filter_map(|e| match e.key {
                Key::Passage(p) => Some(p.start()),
                _ => None,
            })
            .collect())
    }

    /// The Bible books reader `reader`'s Resource has, in its order, with their chapter
    /// counts in its own numbering (none for a Lexicon or Dictionary).
    pub fn books(&self, reader: ReaderId) -> Result<Vec<BookChapters>, MobileError> {
        let mut books: Vec<Book> = Vec::new();
        for v in self.grid_verses(reader)? {
            if books.last() != Some(&v.book) && !books.contains(&v.book) {
                books.push(v.book);
            }
        }
        let mut out = Vec::with_capacity(books.len());
        for b in books {
            out.push(BookChapters {
                id: b.osis().to_string(),
                name: b.name().to_string(),
                chapters: self.chapters(reader, b.osis())?.len() as u16,
            });
        }
        Ok(out)
    }

    fn book(id: &str) -> Result<Book, MobileError> {
        Book::from_osis(id).ok_or_else(|| MobileError::Reference {
            reference: id.to_string(),
            error: "no such book".into(),
        })
    }

    /// The chapters of book `book` (an OSIS id from [`Self::books`]) that reader `reader`'s
    /// Resource has.
    pub fn chapters(&self, reader: ReaderId, book: &str) -> Result<Vec<u16>, MobileError> {
        let b = Self::book(book)?;
        let mut out: Vec<u16> = (self.grid_verses(reader)?.into_iter())
            .filter(|v| v.book == b)
            .map(|v| v.chapter)
            .collect();
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// The verses of chapter `chapter` of book `book` that reader `reader`'s Resource has: a
    /// Bible's in its own numbering, a Commentary's the first Verse of each section there.
    pub fn verses(
        &self,
        reader: ReaderId,
        book: &str,
        chapter: u16,
    ) -> Result<Vec<u16>, MobileError> {
        let b = Self::book(book)?;
        let mut out: Vec<u16> = (self.grid_verses(reader)?.into_iter())
            .filter(|v| v.book == b && v.chapter == chapter)
            .map(|v| v.verse)
            .collect();
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// Go to verse `verse` (its chapter's first without one) of chapter `chapter` of book
    /// `book` from the grid: a jump, as a typed reference is.
    pub fn go_to_verse(
        &mut self,
        reader: ReaderId,
        book: &str,
        chapter: u16,
        verse: Option<u16>,
    ) -> Result<ReaderSnapshot, MobileError> {
        let b = Self::book(book)?;
        let first = match verse {
            Some(n) => Some(n),
            None => self.verses(reader, book, chapter)?.first().copied(),
        };
        let target = first
            .map(|n| Verse::new(b, chapter, n))
            .filter(|v| (self.grid_verses(reader)).is_ok_and(|all| all.contains(v)));
        let Some(target) = target else {
            return Err(MobileError::Reference {
                reference: format!("{} {chapter}", b.name()),
                error: "not in this Resource".into(),
            });
        };
        let tab = self.reader_at(reader)?.tab;
        self.session.workspace.navigate(tab, target);
        self.apply_effects();
        self.reader(reader)
    }

    /// History, newest day first (the shell lists it; [`Self::open_visit`] opens a row).
    pub fn history(&self) -> Vec<HistoryDay> {
        (self.session.history_days("").into_iter())
            .map(|d| HistoryDay {
                heading: d.heading,
                visits: (d.visits.into_iter())
                    .map(|v| HistoryVisit {
                        id: v.id,
                        label: v.label,
                        time: v.time,
                    })
                    .collect(),
            })
            .collect()
    }

    /// Open History's Visit `visit` in reader `reader`: its Resource, at its place, as a
    /// jump. Visits of what the Mobile app has no reader for (a Guide, a Search) are refused.
    pub fn open_visit(
        &mut self,
        reader: ReaderId,
        visit: i64,
    ) -> Result<ReaderSnapshot, MobileError> {
        let location = (self.session.history().iter())
            .find(|v| v.id == visit)
            .map(|v| (v.label.clone(), v.location.clone()))
            .ok_or_else(|| MobileError::Failed(format!("no Visit {visit} in History")))?;
        let (label, location) = location;
        let TabKind::Resource(id) = &location.kind else {
            return Err(MobileError::Failed(format!(
                "“{label}” does not open in a reader"
            )));
        };
        if self.session.library.get(id).is_none() {
            return Err(MobileError::Failed(format!(
                "{id} is no longer in your library"
            )));
        }
        self.open(reader, id, None)?;
        let tab = self.reader_at(reader)?.tab;
        if let Some(key) = location.key {
            self.session.workspace.navigate_entry(tab, key);
        } else if let Some(v) = location.verse {
            self.session.workspace.navigate(tab, v);
        }
        self.apply_effects();
        self.reader(reader)
    }
}
