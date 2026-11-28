//! Find in Resource (#175) on GPUI (#381): a Resource Tab's Find bar finds every match of what
//! is typed in the whole Resource on a worker thread, with `obs-search`'s [`Finder`] (the
//! Search normalizer, so Hebrew typed without points and Greek without accents match), and the
//! reading pane paints them over the laid-out clusters, so they follow scrolling, text size and
//! width exactly like the selection does.
//!
//! The egui app's `find.rs` without egui: the bar's state, the worker and stepping are the same;
//! the pane asks for frames while the worker runs instead of being repainted by it. Text
//! Comparison rows come with the Text Comparison Tool (#391).

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use obs_reading::{DocPos, ReadingDoc, ReadingView};
use obs_search::{FindOptions, Finder};

/// What the empty field says (the egui Find bar's `FIND_HINT`).
pub const FIND_HINT: &str = "Find in this Resource";

/// A selection longer than this (or over several lines) is not put in the Find bar.
pub const SELECTION_LIMIT: usize = 100;

/// One match: `unit` is an index into the document's units, `range` bytes of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    pub unit: usize,
    pub range: Range<usize>,
}

impl FindMatch {
    pub fn start(&self) -> DocPos {
        DocPos {
            unit: self.unit,
            byte: self.range.start,
        }
    }

    pub fn end(&self) -> DocPos {
        DocPos {
            unit: self.unit,
            byte: self.range.end,
        }
    }
}

/// Every match in `doc`, in order; stops early (with what it has) once `cancel` is set.
fn find_in(doc: &ReadingDoc, finder: &Finder, cancel: &AtomicBool) -> Vec<FindMatch> {
    let mut out = Vec::new();
    for (unit, u) in doc.units.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        out.extend(
            finder
                .find(&u.text)
                .into_iter()
                .map(|range| FindMatch { unit, range }),
        );
    }
    out
}

/// Finding in progress, on its own thread.
struct FindJob {
    rx: Receiver<Vec<FindMatch>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for FindJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// What the Find bar shows beside its field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindStatus {
    /// Nothing typed.
    Empty,
    Finding,
    Found {
        current: Option<usize>,
        total: usize,
    },
}

/// The count beside the field (the egui Find bar's `status_text`).
pub fn status_text(status: FindStatus) -> String {
    match status {
        FindStatus::Empty => String::new(),
        FindStatus::Finding => "Finding…".into(),
        FindStatus::Found { total: 0, .. } => "No matches".into(),
        FindStatus::Found {
            current: Some(c),
            total,
        } => format!("{} of {total}", c + 1),
        FindStatus::Found {
            current: None,
            total,
        } => format!("{total} matches"),
    }
}

/// One Tab's Find bar (Ctrl+F): what is typed, its options and the matches.
#[derive(Default)]
pub struct FindBar {
    pub text: String,
    pub options: FindOptions,
    /// Matches of `searched`, in document order.
    matches: Arc<Vec<FindMatch>>,
    pub current: Option<usize>,
    /// What `matches` (or the job) are for: the text, the options and the document.
    searched: Option<(String, FindOptions, usize)>,
    job: Option<FindJob>,
    /// Put the keyboard in the field (and select its text) when the Tab next draws.
    pub focus: bool,
    /// Step to the next (true) or previous match when the pane next draws.
    pub step: Option<bool>,
}

impl FindBar {
    /// Every match, in document order.
    pub fn matches(&self) -> &[FindMatch] {
        &self.matches
    }

    pub fn status(&self) -> FindStatus {
        if self.text.trim().is_empty() {
            FindStatus::Empty
        } else if self.job.is_some() {
            FindStatus::Finding
        } else {
            FindStatus::Found {
                current: self.current,
                total: self.matches.len(),
            }
        }
    }

    /// The worker is still finding.
    pub fn is_finding(&self) -> bool {
        self.job.is_some()
    }

    /// The indexes of the matches of `unit` that reach into its bytes `bytes`.
    pub fn within(&self, unit: usize, bytes: Range<usize>) -> Range<usize> {
        let from = self
            .matches
            .partition_point(|m| (m.unit, m.range.end) <= (unit, bytes.start));
        let to = self
            .matches
            .partition_point(|m| (m.unit, m.range.start) < (unit, bytes.end));
        from..to.max(from)
    }

    /// Find again if what is typed, the options or the document changed since the last search,
    /// on a new thread.
    fn refresh(&mut self, doc: &Arc<ReadingDoc>) {
        let key = Arc::as_ptr(doc) as usize;
        let wanted = (self.text.clone(), self.options, key);
        if self.searched.as_ref() == Some(&wanted) {
            return;
        }
        self.searched = Some(wanted);
        self.job = None;
        self.matches = Arc::new(Vec::new());
        self.current = None;
        let Some(finder) = Finder::new(&self.text, self.options) else {
            return;
        };
        let doc = Arc::clone(doc);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let flag = Arc::clone(&cancel);
        // Should the thread not start, its sender is dropped and nothing is found.
        let _ = std::thread::Builder::new()
            .name("obs-find".into())
            .spawn(move || {
                let found = find_in(&doc, &finder, &flag);
                if !flag.load(Ordering::Relaxed) {
                    let _ = tx.send(found);
                }
            });
        self.job = Some(FindJob { rx, cancel });
    }

    /// Take the matches once the worker has them. True when they arrived just now.
    fn poll(&mut self) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        match job.rx.try_recv() {
            Ok(found) => {
                self.matches = Arc::new(found);
                self.job = None;
                true
            }
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                true
            }
            Err(TryRecvError::Empty) => false,
        }
    }

    /// The match `forward` (or back) from the current one, wrapping around; with none
    /// current, the first at or after `from` (or the last before it).
    fn stepped(&self, forward: bool, from: (usize, usize)) -> Option<usize> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        Some(match self.current {
            Some(c) if forward => (c + 1) % n,
            Some(c) => (c + n - 1) % n,
            None => {
                let at = self.first_from(from);
                if forward { at } else { (at + n - 1) % n }
            }
        })
    }

    /// The first match at or after `(unit, byte)`, wrapping to the first of all.
    fn first_from(&self, (unit, byte): (usize, usize)) -> usize {
        let i = self
            .matches
            .partition_point(|m| (m.unit, m.range.start) < (unit, byte));
        if i < self.matches.len() { i } else { 0 }
    }

    /// Find in the view's text, then show the current match: the first at or after the top of
    /// the view when new matches arrive, the next or previous one on a step. A match out of
    /// view is scrolled to the top of a viewport `viewport` tall. True when the view moved.
    pub fn update(&mut self, view: &mut ReadingView, viewport: f32) -> bool {
        let doc = Arc::clone(&view.doc);
        self.refresh(&doc);
        let mut top = || {
            let at = view.text_anchor();
            at.map_or((0, 0), |a| (a.unit, a.byte))
        };
        let arrived = self.poll();
        let show = if arrived && !self.matches.is_empty() {
            Some(self.first_from(top()))
        } else if let Some(forward) = self.step.take() {
            self.stepped(forward, top())
        } else {
            None
        };
        let Some(i) = show else {
            return false;
        };
        self.current = Some(i);
        let Some(m) = self.matches.get(i) else {
            return false;
        };
        let size = view.settings().size;
        let shown = view
            .text_top(m.unit, m.range.start, viewport)
            .is_some_and(|y| y >= 0.0 && y + 2.0 * size <= viewport);
        !shown && view.jump_to_text(m.unit, m.range.start)
    }
}

/// What Ctrl+F puts in the bar from the selection: its text when it is short and on one line.
pub fn selection_text(view: &ReadingView) -> Option<String> {
    let sel = view.selection.filter(|s| !s.is_empty())?;
    let text = obs_reading::selected_text(&view.doc, &sel);
    let text = text.trim().to_string();
    (!text.is_empty() && !text.contains('\n') && text.chars().count() <= SELECTION_LIMIT)
        .then_some(text)
}
