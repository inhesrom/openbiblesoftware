//! The readers: at most [`MAX_READERS`] (the reader, and the second of a Split view), each one
//! reading Tab of the engine's Workspace in one Panel, with the [`ReadingView`] that lays its
//! Resource out. The Workspace keeps each reader's Resource, Current location and Back/Forward
//! history, and runs the commands (`resource.open`, `tab.go`, `tab.back`, `tab.parallel`) as
//! the desktop's do; this module loads the views and places them where the Workspace says.
//! Tab and Panel ids never leave the crate: the shells know readers.

use std::sync::Arc;

use obs_engine::{TabId, TabKind};
use obs_model::{Key, ResourceType};
use obs_reading::{HighlightLooks, MarkKind, ReadingView};
use obs_scripture::{Passage, RefStyle, Verse, format_passage};
use serde_json::json;

use crate::app::failed;
use crate::{MobileApp, MobileError};

/// The most readers at once: the reader and the second reader of a Split view.
pub const MAX_READERS: usize = 2;

/// One reader of the running app: 0 is the reader, 1 the second reader of a Split view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReaderId(pub u32);

/// What a reader shows, for its bar and its menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderSnapshot {
    pub reader: ReaderId,
    pub resource_id: String,
    pub abbreviation: String,
    pub title: String,
    pub resource_type: ResourceType,
    /// The Resource's script runs right to left (Hebrew).
    pub right_to_left: bool,
    /// The reader goes to entry Keys (a Lexicon's Strong's numbers, a Dictionary's
    /// headwords) rather than Bible references.
    pub keyed: bool,
    /// The Current location as the bar shows it: `John 3:16`, or the entry's Key (`G26`).
    pub location: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// One reader: its Tab, the view of its Resource and what the text view keeps for it.
#[derive(Debug)]
pub(crate) struct Reader {
    pub(crate) tab: TabId,
    pub(crate) view: ReadingView,
    pub(crate) frame: crate::frame::FrameState,
    /// The annotation revision its Note markers' room was made at.
    pub(crate) notes_revision: Option<u64>,
}

impl Reader {
    pub(crate) fn new(tab: TabId, view: ReadingView) -> Self {
        Self {
            tab,
            view,
            frame: crate::frame::FrameState::new(),
            notes_revision: None,
        }
    }

    /// Whether it shows Resource `id`.
    pub(crate) fn shows(&self, id: &str) -> bool {
        self.view.doc.manifest.resource_id == id
    }
}

/// The Current location of a reader, as its bar names it: a Verse in the Resource's own
/// numbering, or an entry Key.
pub(crate) fn location_label(
    view: &ReadingView,
    verse: Option<Verse>,
    key: Option<&Key>,
) -> String {
    if let Some(key) = key {
        return key.to_string();
    }
    verse.map_or_else(String::new, |v| {
        format_passage(Passage::verse(v), &view.doc.versification, RefStyle::Long)
    })
}

impl MobileApp {
    pub(crate) fn reader_at(&self, reader: ReaderId) -> Result<&Reader, MobileError> {
        (self.readers.get(reader.0 as usize)).ok_or(MobileError::NoReader(reader))
    }

    pub(crate) fn reader_at_mut(&mut self, reader: ReaderId) -> Result<&mut Reader, MobileError> {
        (self.readers.get_mut(reader.0 as usize)).ok_or(MobileError::NoReader(reader))
    }

    /// The reader that shows Tab `tab`.
    pub(crate) fn reader_of(&self, tab: TabId) -> Option<usize> {
        self.readers.iter().position(|r| r.tab == tab)
    }

    /// Every reader, first one first.
    pub fn readers(&self) -> Vec<ReaderSnapshot> {
        (0..self.readers.len())
            .filter_map(|i| self.reader(ReaderId(i as u32)).ok())
            .collect()
    }

    /// What reader `reader` shows.
    pub fn reader(&self, reader: ReaderId) -> Result<ReaderSnapshot, MobileError> {
        let r = self.reader_at(reader)?;
        let manifest = &r.view.doc.manifest;
        let tab = self.session.workspace.tab(r.tab);
        let keyed = tab.is_some_and(obs_engine::Tab::keeps_entries);
        let (verse, key) = match tab {
            Some(t) if keyed => (None, t.entry_history.current()),
            Some(t) => (t.history.current(), None),
            None => (None, None),
        };
        let history = |t: &obs_engine::Tab| {
            if keyed {
                (
                    t.entry_history.can_go_back(),
                    t.entry_history.can_go_forward(),
                )
            } else {
                (t.history.can_go_back(), t.history.can_go_forward())
            }
        };
        let (can_go_back, can_go_forward) = tab.map_or((false, false), history);
        Ok(ReaderSnapshot {
            reader,
            resource_id: manifest.resource_id.clone(),
            abbreviation: manifest.abbreviation.clone(),
            title: manifest.title.clone(),
            resource_type: manifest.resource_type,
            right_to_left: r.view.doc.direction() == obs_model::Direction::Rtl,
            keyed,
            location: location_label(&r.view, verse, key.as_ref()),
            can_go_back,
            can_go_forward,
        })
    }

    /// Show Resource `resource_id` in reader `reader`, at `at` (a Bible reference read in the
    /// Resource's own numbering, or an entry Key) when given. A reader that does not exist
    /// yet is opened, the next one only (0, then 1); one that shows another Resource of the
    /// same type switches to it as the Parallel resources switch does, at the same place and
    /// as a step in its Back/Forward history; one of another type starts afresh. Without `at`
    /// the reader stays where it was, and a new one starts at the Resource's start.
    pub fn open(
        &mut self,
        reader: ReaderId,
        resource_id: &str,
        at: Option<&str>,
    ) -> Result<ReaderSnapshot, MobileError> {
        let wanted = (self.session.library.get(resource_id))
            .map(|e| e.manifest.resource_type)
            .ok_or_else(|| MobileError::NoResource(resource_id.to_string()))?;
        let i = reader.0 as usize;
        if i > self.readers.len() || i >= MAX_READERS {
            return Err(MobileError::NoReader(reader));
        }
        // A Resource that cannot be read changes nothing.
        self.doc(resource_id)?;
        if i == self.readers.len() {
            let tab = self.open_tab(resource_id)?;
            let view = self.load_view(tab)?;
            self.readers.push(Reader::new(tab, view));
        } else if self.readers[i].view.doc.manifest.resource_id != resource_id {
            let old = self.readers[i].tab;
            if self.readers[i].view.doc.manifest.resource_type == wanted {
                (self
                    .session
                    .run("tab.parallel", &json!({ "tab": old.0, "id": resource_id })))
                .map_err(failed)?;
            } else {
                let tab = self.open_tab(resource_id)?;
                let view = self.load_view(tab)?;
                self.readers[i] = Reader::new(tab, view);
                self.new_epoch();
                (self.session.run("tab.close", &json!({ "tab": old.0 }))).map_err(failed)?;
            }
        }
        self.apply_effects();
        if let Some(text) = at {
            self.navigate(reader, text)?;
        }
        self.reader(reader)
    }

    /// Open a Tab of Resource `resource_id` (in the Workspace's one reading Panel).
    pub(crate) fn open_tab(&mut self, resource_id: &str) -> Result<TabId, MobileError> {
        let out = (self
            .session
            .run("resource.open", &json!({ "id": resource_id })))
        .map_err(failed)?;
        (out["tab"].as_u64())
            .map(TabId)
            .ok_or_else(|| MobileError::Failed("resource.open gave no Tab".into()))
    }

    /// The view of Tab `tab`'s Resource, placed where the Tab is (a parallel resource's
    /// arrival, or its own location), else at the Resource's start; the Tab learns the
    /// Resource's numbering, navigation and Key schemes, as the desktop's pane does when it
    /// loads a Resource.
    pub(crate) fn load_view(&mut self, tab: TabId) -> Result<ReadingView, MobileError> {
        let id = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Resource(id)) => id.clone(),
            _ => {
                return Err(MobileError::Failed(format!(
                    "Tab {} shows no Resource",
                    tab.0
                )));
            }
        };
        let doc = self.doc(&id)?;
        let mut view = ReadingView::with_estimate_threads(
            Arc::clone(&doc),
            self.session.text_settings_of(&id),
            self.estimate_threads,
        );
        let ty = doc.manifest.resource_type;
        let ws = &mut self.session.workspace;
        if let Some(t) = ws.tab_mut(tab) {
            t.versification =
                (ty == ResourceType::Bible).then(|| Arc::new(doc.versification.clone()));
            if ty != ResourceType::Bible {
                t.navigation = Some(Arc::clone(&doc.navigation));
            }
            t.verse_map = doc.verse_map.clone();
            if !doc.manifest.key_schemes.is_empty() {
                t.key_schemes = doc.manifest.key_schemes.clone();
            }
        }
        // A switch to a parallel resource goes to the same place (#151); otherwise the Tab's
        // own location (the Last session's), else the Resource's start: a Bible that lacks
        // the place (another revision, a damaged Last session) is at its first Verse.
        let arrived = ws.arrival(tab);
        let own = ws.tab(tab).and_then(|t| t.history.current());
        let first_verse = (doc.verses.first())
            .map(|f| f.verse)
            .filter(|_| ty == ResourceType::Bible);
        if let Some(v) = arrived.or(own) {
            let local = if ty == ResourceType::Commentary {
                (doc.navigation.section(v)).and_then(|e| match e.key {
                    Key::Passage(p) => Some(p.start()),
                    _ => None,
                })
            } else {
                Some(v)
            };
            if let Some(v) = local
                && view.jump_to(v)
            {
                ws.scrolled(tab, v);
            } else if let Some(first) = first_verse {
                ws.scrolled(tab, first);
            }
        } else if let Some(first) = first_verse {
            ws.scrolled(tab, first);
        }
        if matches!(ty, ResourceType::Lexicon | ResourceType::Dictionary) {
            let key = (ws.tab(tab).and_then(|t| t.entry_history.current()))
                .and_then(|k| doc.navigation.lookup(&k).map(|e| e.key.clone()))
                .or_else(|| doc.navigation.entries().next().map(|e| e.key.clone()));
            if let Some(key) = key
                && view.jump_to_entry(&key)
                && let Some(t) = ws.tab_mut(tab)
            {
                t.entry_history.scrolled_to(key);
            }
        }
        Ok(view)
    }

    /// Load reader `i`'s view again: its Tab now shows another Resource, or a new revision of
    /// its own. The shell takes the new view's position.
    pub(crate) fn reload(&mut self, i: usize) {
        let Some(tab) = self.readers.get(i).map(|r| r.tab) else {
            return;
        };
        match self.load_view(tab) {
            Ok(view) => {
                self.readers[i].view = view;
                // The old window's blocks are another document's: until the next frame lays
                // this one out, the text calls find no text (a new FrameState is `moved`).
                self.readers[i].frame = crate::frame::FrameState::new();
                self.readers[i].notes_revision = None;
                // Nor may any tile key of before be reused.
                self.new_epoch();
            }
            Err(e) => self.events.message(e.to_string()),
        }
    }

    /// Make room in reader `i`'s text for the markers of the Notes that show in it (#305), as
    /// of the User database's annotation revision now.
    pub(crate) fn refresh_note_room(&mut self, i: usize) {
        let revision = self.session.annotations_revision();
        let Some(r) = self.readers.get_mut(i) else {
            return;
        };
        if r.notes_revision == Some(revision) {
            return;
        }
        r.notes_revision = Some(revision);
        let id = &r.view.doc.manifest.resource_id;
        // While its Annotations are re-anchored they may point at text that moved (#65).
        if self.session.user.is_none() || self.session.is_reanchoring(id) {
            r.view.set_note_ends(Vec::new());
            return;
        }
        let annotations = self.session.annotations_in_text().unwrap_or_default();
        let marks = obs_reading::marks_for(&r.view.doc, &annotations, &HighlightLooks::built_in());
        r.view.set_note_ends(
            (marks.iter())
                .filter(|m| m.kind == MarkKind::Note)
                .map(|m| m.end)
                .collect(),
        );
    }
}
