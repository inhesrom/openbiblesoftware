//! Events (plan A3): what changed in the background, for the shell to show. The Mobile app has
//! no frame loop, so the shell's pump (obs-ffi's thread) calls [`MobileApp::pump`] on a timer:
//! it lets the engine route finished jobs (`Session::poll_imports`, which alone reads the job
//! queue's events), applies the Workspace's effects to the readers and reports each change
//! once as a [`Changed`].
//!
//! **Effects.** The Mobile app runs only the reading commands (`resource.open`, `tab.go`,
//! `tab.back`, `tab.forward`, `tab.parallel`, `tab.close`) and the View menu's settings, so
//! the effects those and the background jobs make are the ones that mean something here:
//! moves of a reader's Tab (`Navigate`, `NavigateEntry`, `NavigateText`, `NavigateOpened`,
//! `NavigateStandard`, `Follow`, `ShowUnit`, `RevealText`), another Resource in it
//! (`Switched`), a Resource replaced or removed (`Replaced`, `Removed`) and a `Message`. The
//! rest open desktop windows, menus or Panels (`Opened`, `Focus`, `Closed`, `Activate` place
//! Tabs in Panels the Mobile app arranges itself): no Mobile command makes them, and they are
//! dropped.

use obs_engine::Effect;

use crate::{MobileApp, ReaderId};

/// One change for the shell to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// The library's Resources changed (a Starter install finished, a Resource left).
    Library,
    /// A reader moved by itself or shows other text (another Resource, a new revision, room
    /// for a Note): ask for its frame again.
    Reader(ReaderId),
    /// History has new Visits or new day headings.
    History,
    /// A short message for the user, in the engine's words (a toast).
    Message(String),
    /// Library → Starter library, Get Resources or a download changed (the released manifest
    /// came in, a download moved on, a Resource was removed): ask for them again.
    Downloads,
    /// An Import's report changed (`MobileApp::import_report`).
    Imports,
}

/// What the pump has reported so far.
#[derive(Debug, Default)]
pub(crate) struct EventState {
    /// Changes made since the last pump, oldest first, each once.
    pending: Vec<Changed>,
    /// The library's Preferred order last reported.
    pub(crate) seen_library: Vec<String>,
    /// The newest Visit last reported.
    pub(crate) seen_history: Option<i64>,
    /// The Starter library's check, error and row count last reported.
    pub(crate) seen_starter: (bool, Option<String>, usize),
}

impl EventState {
    pub(crate) fn push(&mut self, change: Changed) {
        if !self.pending.contains(&change) {
            self.pending.push(change);
        }
    }

    pub(crate) fn message(&mut self, text: String) {
        self.push(Changed::Message(text));
    }
}

/// Effects that send a Tab somewhere else can make more (a Tab opened at a place jumps
/// there): they are applied this many rounds at most.
const EFFECT_ROUNDS: usize = 8;

impl MobileApp {
    /// What changed since the last call: finished jobs routed to their owners, effects applied
    /// to the readers, History's new Visits recorded. With jobs inline (tests), it runs a few
    /// job steps first.
    pub fn pump(&mut self) -> Vec<Changed> {
        if !self.session.jobs().is_threaded() {
            self.session.jobs().pump(64);
        }
        // The other events are the engine's own (imports, indexes, Guides); it routes them.
        let _ = self.session.poll_imports();
        self.apply_effects();
        self.session.record_history();
        let newest = self.session.history().first().map(|v| v.id);
        if newest != self.events.seen_history {
            self.events.seen_history = newest;
            self.events.push(Changed::History);
        }
        self.advance_downloads();
        self.advance_imports();
        let seen = self.starter_seen();
        if seen != self.events.seen_starter {
            self.events.seen_starter = seen;
            self.events.push(Changed::Downloads);
        }
        let order = self.session.library.preferred_order();
        if order != self.events.seen_library {
            self.events.seen_library = order;
            self.events.push(Changed::Library);
        }
        let revision = self.session.annotations_revision();
        for i in 0..self.readers.len() {
            if self.readers[i]
                .notes_revision
                .is_some_and(|r| r != revision)
            {
                self.refresh_note_room(i);
                self.events.push(Changed::Reader(ReaderId(i as u32)));
            }
        }
        std::mem::take(&mut self.events.pending)
    }

    /// The Starter library's check, error and row count, to tell when they change.
    pub(crate) fn starter_seen(&self) -> (bool, Option<String>, usize) {
        let starter = &self.session.starter;
        (
            starter.is_checking(),
            starter.error().map(str::to_string),
            starter.manifest().map_or(0, |m| m.resources.len()),
        )
    }

    /// Whether background jobs are running (the pump polls more often meanwhile).
    pub fn busy(&self) -> bool {
        self.session.jobs().is_busy()
    }

    /// Apply the Workspace's effects to the readers (see the module docs for which mean
    /// something here).
    pub(crate) fn apply_effects(&mut self) {
        for _ in 0..EFFECT_ROUNDS {
            let effects = self.session.workspace.take_effects();
            if effects.is_empty() {
                return;
            }
            for e in effects {
                self.apply_effect(e);
            }
        }
    }

    fn apply_effect(&mut self, e: Effect) {
        let moved = |app: &mut Self, i: usize| {
            app.readers[i].frame.moved = true;
            app.events.push(Changed::Reader(ReaderId(i as u32)));
        };
        match e {
            Effect::Navigate { tab, verse } | Effect::Follow { tab, verse, .. } => {
                let Some(i) = self.reader_of(tab) else {
                    return;
                };
                if self.readers[i].view.jump_to(verse) {
                    moved(self, i);
                } else {
                    self.events
                        .message(format!("No section or Verse for {verse} in this Resource"));
                }
            }
            Effect::NavigateEntry { tab, key } => {
                if let Some(i) = self.reader_of(tab)
                    && self.readers[i].view.jump_to_entry(&key)
                {
                    moved(self, i);
                }
            }
            Effect::NavigateText {
                tab,
                unit,
                range,
                key,
            } => {
                let Some(i) = self.reader_of(tab) else {
                    return;
                };
                let view = &mut self.readers[i].view;
                let index = view.doc.units.iter().position(|u| u.id == unit);
                let shown = match index {
                    Some(u) if view.jump_to_text(u, range.start as usize) => true,
                    // The text is not shown (a note): its Key, at least.
                    _ => match &key {
                        Some(obs_model::Key::Passage(p)) => view.jump_to(p.start()),
                        Some(k) => view.jump_to_entry(k),
                        None => false,
                    },
                };
                if shown {
                    moved(self, i);
                } else {
                    self.events
                        .message(format!("“{unit}” is not shown in this Resource"));
                }
            }
            Effect::ShowUnit { tab, unit } => {
                let Some(i) = self.reader_of(tab) else {
                    return;
                };
                let view = &mut self.readers[i].view;
                if view.doc.manifest.resource_type == obs_model::ResourceType::Bible {
                    return;
                }
                if let Some(u) = view.doc.units.iter().position(|x| x.id == unit)
                    && view.top_unit() != Some(u)
                    && view.jump_to_text(u, 0)
                {
                    moved(self, i);
                }
            }
            Effect::RevealText { tab, unit, byte } => {
                let Some(i) = self.reader_of(tab) else {
                    return;
                };
                let view = &mut self.readers[i].view;
                let index = view.doc.units.iter().position(|x| x.id == unit);
                if index.is_some_and(|u| view.jump_to_pos(u, byte)) {
                    moved(self, i);
                } else {
                    self.events
                        .message("That text is no longer in this Resource".into());
                }
            }
            Effect::NavigateOpened { tab, verse, key } => {
                let ws = &mut self.session.workspace;
                if let Some(v) = verse {
                    ws.navigate(tab, v);
                }
                if let Some(key) = key {
                    ws.navigate_entry(tab, key);
                }
            }
            Effect::NavigateStandard { tab, verse } => {
                self.session.workspace.navigate_standard(tab, verse);
            }
            Effect::Switched(tab) => {
                if let Some(i) = self.reader_of(tab) {
                    self.reload(i);
                    moved(self, i);
                }
            }
            Effect::Replaced(id) => {
                self.docs.remove(&id);
                let showing: Vec<usize> = (0..self.readers.len())
                    .filter(|i| self.readers[*i].view.doc.manifest.resource_id == id)
                    .collect();
                for i in showing {
                    self.reload(i);
                    moved(self, i);
                }
                self.events.push(crate::Changed::Library);
            }
            Effect::Removed(id) => {
                self.docs.remove(&id);
                self.events.push(crate::Changed::Library);
            }
            Effect::Message(text) => self.events.message(text),
            // Placing Tabs in Panels, desktop windows, menus and dialogs (module docs).
            _ => {}
        }
    }
}
