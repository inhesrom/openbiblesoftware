//! A Resource's Contents (#176): the outline its Contents pane shows beside the text. A
//! Bible's Bible books and chapters; a Commentary's volumes, Bible books, chapters and
//! sections; a Lexicon's or Dictionary's entries under their first letter, A–Z, with a filter.
//!
//! Built from what the `.obr` already holds (the verse list, Keys, unit titles and a
//! Commentary's volumes), once per Resource, on a worker; nothing here reads a file.

use std::collections::{BTreeMap, HashMap};

use obs_model::{BibleVerse, Key, KeyEntry, ResourceType, TextUnit, UnitId, normalize};
use obs_scripture::{Book, Passage, Standard, Verse};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::resource_navigation::ResourceNavigation;
use crate::{Session, TabId, TabKind, Workspace};

/// Where clicking a Contents entry goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentsTarget {
    /// A Bible Verse, in the Bible's own numbering.
    Verse(Verse),
    /// A Commentary section, or a Lexicon or Dictionary entry: its Key and unit.
    Entry(KeyEntry),
}

/// One line of the Contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsNode {
    pub label: String,
    /// 0 for a top-level line.
    pub depth: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Where clicking it goes (a group goes to its first entry).
    pub target: Option<ContentsTarget>,
}

/// The outline of one Resource: nodes in Contents order, each parent before its children.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contents {
    nodes: Vec<ContentsNode>,
    roots: Vec<usize>,
    /// A Bible's chapter lines, by Bible book and chapter (own numbering).
    chapters: HashMap<(Book, u16), usize>,
    /// The line of each Commentary section or entry unit.
    units: HashMap<UnitId, usize>,
    /// The entries the filter box searches: line and normalized label, in Contents order.
    entries: Vec<(usize, String)>,
}

impl Contents {
    /// The Contents of a Resource of type `ty`. `units` are its text units in document order
    /// (titles name a Lexicon's or Dictionary's entries); `volumes` a Commentary's volume per
    /// Bible book.
    pub fn build(
        ty: ResourceType,
        verses: &[BibleVerse],
        units: &[TextUnit],
        navigation: &ResourceNavigation,
        volumes: &BTreeMap<Book, String>,
    ) -> Self {
        match ty {
            ResourceType::Bible => Self::bible(verses),
            ResourceType::Commentary => Self::commentary(navigation, volumes),
            ResourceType::Lexicon | ResourceType::Dictionary => Self::entries(units, navigation),
            _ => Self::default(),
        }
    }

    /// Bible books, each with its chapters, in document order.
    pub fn bible(verses: &[BibleVerse]) -> Self {
        let mut c = Self::default();
        let mut books: HashMap<Book, usize> = HashMap::new();
        for v in verses {
            let at = v.verse;
            let book = match books.get(&at.book) {
                Some(&n) => n,
                None => {
                    let n = c.push(None, at.book.name().into(), Some(ContentsTarget::Verse(at)));
                    books.insert(at.book, n);
                    n
                }
            };
            if !c.chapters.contains_key(&(at.book, at.chapter)) {
                let label = format!("Chapter {}", at.chapter);
                let n = c.push(Some(book), label, Some(ContentsTarget::Verse(at)));
                c.chapters.insert((at.book, at.chapter), n);
            }
        }
        c
    }

    /// Volumes (when the Commentary names them), Bible books, chapters and sections, in
    /// canonical order. A book's introduction comes first in the book, a chapter's first in
    /// the chapter.
    pub fn commentary(navigation: &ResourceNavigation, volumes: &BTreeMap<Book, String>) -> Self {
        let mut sections: Vec<(Passage, &KeyEntry, Level)> = navigation
            .keys
            .iter()
            .filter(|e| !e.alias)
            .filter_map(|e| match e.key {
                Key::Passage(p) => Some((p, e, Level::of(p, &e.unit))),
                _ => None,
            })
            .collect();
        sections.sort_by(|(a, ea, la), (b, eb, lb)| {
            let order = |p: &Passage, level: Level| {
                let chapter = if level == Level::Book {
                    0
                } else {
                    p.start().chapter
                };
                (
                    p.start().book,
                    chapter,
                    level == Level::Verses,
                    p.start(),
                    p.end(),
                )
            };
            order(a, *la)
                .cmp(&order(b, *lb))
                .then_with(|| ea.unit.cmp(&eb.unit))
        });
        let mut c = Self::default();
        let mut volume: Option<(&str, usize)> = None;
        let mut volume_lines = Vec::new();
        let mut books: HashMap<Book, usize> = HashMap::new();
        let mut chapters: HashMap<(Book, u16), usize> = HashMap::new();
        for (p, entry, level) in sections {
            let start = p.start();
            let target = || Some(ContentsTarget::Entry(entry.clone()));
            let book = match books.get(&start.book) {
                Some(&n) => n,
                None => {
                    let parent = match volumes.get(&start.book) {
                        Some(title) => match volume {
                            Some((shown, n)) if shown == title => Some(n),
                            _ => {
                                let n = c.push(None, title.clone(), target());
                                volume = Some((title.as_str(), n));
                                volume_lines.push(n);
                                Some(n)
                            }
                        },
                        None => {
                            volume = None;
                            None
                        }
                    };
                    let n = c.push(parent, start.book.name().into(), target());
                    books.insert(start.book, n);
                    n
                }
            };
            let parent = if level == Level::Book {
                book
            } else {
                match chapters.get(&(start.book, start.chapter)) {
                    Some(&n) => n,
                    None => {
                        let label = format!("Chapter {}", start.chapter);
                        let n = c.push(Some(book), label, target());
                        chapters.insert((start.book, start.chapter), n);
                        n
                    }
                }
            };
            let n = c.push(Some(parent), section_label(p, level, &entry.unit), target());
            c.units.entry(entry.unit.clone()).or_insert(n);
        }
        // "Volume 1" becomes "Volume 1 (Genesis–Deuteronomy)": the Bible books it holds.
        for n in volume_lines {
            let children = &c.nodes[n].children;
            let (Some(&first), Some(&last)) = (children.first(), children.last()) else {
                continue;
            };
            let books = if first == last {
                c.nodes[first].label.clone()
            } else {
                format!("{}–{}", c.nodes[first].label, c.nodes[last].label)
            };
            c.nodes[n].label = format!("{} ({books})", c.nodes[n].label);
        }
        c
    }

    /// Entries in the Resource's own order, under the first letter of their headword or
    /// Lemma. A Lexicon entry shows its Strong's number after the Lemma.
    pub fn entries(units: &[TextUnit], navigation: &ResourceNavigation) -> Self {
        let mut c = Self::default();
        let mut letters: HashMap<String, usize> = HashMap::new();
        for unit in units {
            let Some(entry) = navigation.entry_for_unit(&unit.id) else {
                continue;
            };
            let key = entry.key.to_string();
            let name = unit
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| key.clone());
            let label = match &entry.key {
                Key::StrongsNumber(_) if name != key => format!("{name} ({key})"),
                _ => name.clone(),
            };
            let letter = normalize(&name)
                .chars()
                .find(|ch| ch.is_alphanumeric())
                .map_or_else(|| "#".to_string(), |ch| ch.to_uppercase().collect());
            let target = Some(ContentsTarget::Entry(entry.clone()));
            let group = match letters.get(&letter) {
                Some(&n) => n,
                None => {
                    let n = c.push(None, letter.clone(), target.clone());
                    letters.insert(letter, n);
                    n
                }
            };
            let n = c.push(Some(group), label.clone(), target);
            c.units.insert(unit.id.clone(), n);
            c.entries.push((n, normalize(&label)));
        }
        c
    }

    fn push(
        &mut self,
        parent: Option<usize>,
        label: String,
        target: Option<ContentsTarget>,
    ) -> usize {
        let n = self.nodes.len();
        let depth = parent.map_or(0, |p| self.nodes[p].depth + 1);
        self.nodes.push(ContentsNode {
            label,
            depth,
            parent,
            children: Vec::new(),
            target,
        });
        match parent {
            Some(p) => self.nodes[p].children.push(n),
            None => self.roots.push(n),
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, n: usize) -> &ContentsNode {
        &self.nodes[n]
    }

    /// The top-level lines.
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// Whether the Contents has a filter box: a Lexicon's or Dictionary's entries.
    pub fn is_filterable(&self) -> bool {
        !self.entries.is_empty()
    }

    /// The entries whose label holds `text` (case and diacritics aside): those starting with
    /// it first, then the rest, each in Contents order.
    pub fn filter(&self, text: &str) -> Vec<usize> {
        let query = normalize(text.trim());
        if query.is_empty() {
            return Vec::new();
        }
        let (mut starting, containing): (Vec<_>, Vec<_>) = self
            .entries
            .iter()
            .filter(|(_, label)| label.contains(&query))
            .partition(|(_, label)| label.starts_with(&query));
        starting.extend(containing);
        starting.into_iter().map(|(n, _)| *n).collect()
    }

    /// A Bible's chapter line for a Verse in its own numbering.
    pub fn at_verse(&self, verse: Verse) -> Option<usize> {
        self.chapters.get(&(verse.book, verse.chapter)).copied()
    }

    /// The line of a Commentary section or entry, by its unit.
    pub fn at_unit(&self, unit: &UnitId) -> Option<usize> {
        self.units.get(unit).copied()
    }

    /// The lines above `n`, nearest first.
    pub fn ancestors(&self, n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut at = self.nodes.get(n).and_then(|node| node.parent);
        while let Some(p) = at {
            out.push(p);
            at = self.nodes[p].parent;
        }
        out
    }
}

/// Where a Commentary section sits: a whole book's introduction, a whole chapter's, or a
/// section of verses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Book,
    Chapter,
    Verses,
}

impl Level {
    /// The StudyImport converters mark introductions in the unit id; a SWORD Commentary keys
    /// a book's or chapter's introduction by the whole book or chapter.
    fn of(p: Passage, unit: &UnitId) -> Self {
        let id = unit.as_str();
        let (start, end) = (p.start(), p.end());
        if id.ends_with(".book-intro")
            || (start.chapter != end.chapter && Passage::book(&Standard, start.book) == Some(p))
        {
            Level::Book
        } else if id.ends_with(".chapter-intro")
            || Passage::chapter(&Standard, start.book, start.chapter) == Some(p)
        {
            Level::Chapter
        } else {
            Level::Verses
        }
    }
}

fn section_label(p: Passage, level: Level, unit: &UnitId) -> String {
    let (start, end) = (p.start(), p.end());
    let verse = |v: u16| {
        if v == 0 {
            "title".to_string()
        } else {
            v.to_string()
        }
    };
    match level {
        Level::Book => "Introduction".into(),
        // A whole chapter is its introduction when the converter says so (MHC), else it is
        // the commentary on the whole chapter.
        Level::Chapter if unit.as_str().ends_with(".chapter-intro") || start.verse == 0 => {
            "Introduction".into()
        }
        Level::Chapter => "Whole chapter".into(),
        Level::Verses if start.book != end.book || start.chapter != end.chapter => format!(
            "{}:{}–{}:{}",
            start.chapter,
            verse(start.verse),
            end.chapter,
            verse(end.verse)
        ),
        Level::Verses if start.verse == end.verse => format!("Verse {}", verse(start.verse)),
        Level::Verses => format!("Verses {}–{}", verse(start.verse), verse(end.verse)),
    }
}

/// Go to a Contents line's target in Tab `tab`, as a jump: it is a step in the Tab's
/// Back/Forward history and a Visit, and its Link set follows. A section or entry that the
/// Key alone would not reach (a book introduction starting where its first section does)
/// is reached by its unit.
pub fn go(ws: &mut Workspace, tab: TabId, target: &ContentsTarget) {
    match target {
        ContentsTarget::Verse(v) => ws.navigate(tab, *v),
        ContentsTarget::Entry(entry) => {
            let reached = ws
                .tab(tab)
                .and_then(|t| t.navigation.as_ref())
                .and_then(|n| {
                    match &entry.key {
                        Key::Passage(p) => n.section(p.start()),
                        k => n.lookup(k),
                    }
                    .map(|e| e.unit == entry.unit)
                });
            match &entry.key {
                Key::Passage(p) if reached == Some(true) => ws.navigate(tab, p.start()),
                k if reached == Some(true) => ws.navigate_entry(tab, k.clone()),
                k => {
                    let at = entry.range.map_or(0, |(start, _)| start);
                    ws.navigate_text(tab, entry.unit.clone(), at..at, Some(k.clone()));
                }
            }
        }
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// `tab.contents`: show or hide a Resource Tab's Contents pane.
fn set_contents(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.contents";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(ID, "no tab"))?;
    let t = s
        .workspace
        .tab_mut(tab)
        .ok_or_else(|| failed(ID, format!("no Tab {}", tab.0)))?;
    if !matches!(t.kind, TabKind::Resource(_)) {
        return Err(failed(ID, "only a Resource Tab has Contents"));
    }
    t.contents = p.bool("on").unwrap_or(!t.contents);
    Ok(json!({ "on": t.contents }))
}

fn always(_: &Session) -> bool {
    true
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "tab.contents",
    label: "Contents",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: true,
            help: "The Resource Tab",
        },
        ParamSpec {
            name: "on",
            kind: ParamKind::Boolean,
            required: false,
            help: "Show (true) or hide (false) the Contents pane; without it, toggle it",
        },
    ],
    enabled: always,
    run: set_contents,
}];
