//! Find in Resource (#175): a Tab's Find bar finds every match of what is typed in the whole
//! Resource (or every row of a Text Comparison) on a worker, and the pane draws them as an
//! overlay over the laid-out clusters, so they follow scrolling, text size and width exactly
//! like the selection does. Matching is [`obs_search::Finder`]'s (the Search normalizer).

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use obs_search::{FindOptions, Finder};

use crate::doc::ReadingDoc;
use crate::selection::DocPos;
use crate::view::ReadingView;

/// One match: in a Resource Tab, `unit` is an index into the document's units and `row` is 0;
/// in a Text Comparison, `unit` is the Verse block and `row` its Bible's row. `range` is bytes
/// of that text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    pub unit: usize,
    pub row: usize,
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

/// What a pane draws: every match (sorted by unit, row and start) and the current one.
#[derive(Debug, Clone, Copy, Default)]
pub struct Found<'a> {
    pub matches: &'a [FindMatch],
    pub current: Option<usize>,
}

impl Found<'_> {
    /// The indexes of the matches of `unit` and `row` that reach into its bytes `bytes`.
    pub fn within(&self, unit: usize, row: usize, bytes: Range<usize>) -> Range<usize> {
        let from = self
            .matches
            .partition_point(|m| (m.unit, m.row, m.range.end) <= (unit, row, bytes.start));
        let to = self
            .matches
            .partition_point(|m| (m.unit, m.row, m.range.start) < (unit, row, bytes.end));
        from..to.max(from)
    }
}

/// The texts a Find bar searches.
pub(crate) enum FindSource {
    /// A Resource's text units.
    Doc(Arc<ReadingDoc>),
    /// A Text Comparison's rows: (Verse block, row, text).
    Rows(Vec<(usize, usize, String)>),
}

impl FindSource {
    /// Every match, in order; stops early (with what it has) once `cancel` is set.
    fn find(&self, finder: &Finder, cancel: &AtomicBool) -> Vec<FindMatch> {
        let mut out = Vec::new();
        match self {
            FindSource::Doc(doc) => {
                for (unit, u) in doc.units.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    out.extend(finder.find(&u.text).into_iter().map(|range| FindMatch {
                        unit,
                        row: 0,
                        range,
                    }));
                }
            }
            FindSource::Rows(rows) => {
                for (unit, row, text) in rows {
                    out.extend(finder.find(text).into_iter().map(|range| FindMatch {
                        unit: *unit,
                        row: *row,
                        range,
                    }));
                }
            }
        }
        out
    }
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

/// What the Find bar shows below its count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FindStatus {
    /// Nothing typed.
    Empty,
    Finding,
    Found {
        current: Option<usize>,
        total: usize,
    },
}

/// One Tab's Find bar (Ctrl+F): what is typed, its options and the matches.
#[derive(Default)]
pub(crate) struct FindBar {
    pub(crate) text: String,
    pub(crate) options: FindOptions,
    /// Matches of `searched`, in document order.
    matches: Arc<Vec<FindMatch>>,
    pub(crate) current: Option<usize>,
    /// What `matches` (or the job) are for: the text, the options and the source's key.
    searched: Option<(String, FindOptions, u64)>,
    job: Option<FindJob>,
    /// Put the keyboard in the field (and select its text) next frame.
    pub(crate) focus: bool,
    /// Step to the next (true) or previous match when the pane next draws.
    pub(crate) step: Option<bool>,
    /// Where ▲ and ▼ were drawn last frame.
    pub(crate) arrows: Option<[egui::Rect; 2]>,
}

impl FindBar {
    pub(crate) fn found(&self) -> Found<'_> {
        Found {
            matches: &self.matches,
            current: self.current,
        }
    }

    pub(crate) fn status(&self) -> FindStatus {
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

    /// Find again if what is typed, the options, or the source (`key`) changed since the last
    /// search: on a new thread (inline on wasm), `ctx` repainting when it is done.
    fn refresh(&mut self, key: u64, source: impl FnOnce() -> FindSource, ctx: &egui::Context) {
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
        let source = source();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let work = {
            let cancel = Arc::clone(&cancel);
            let ctx = ctx.clone();
            move || {
                let found = source.find(&finder, &cancel);
                if !cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(found);
                    ctx.request_repaint();
                }
            }
        };
        // Should the thread not start, its sender is dropped and nothing is found.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::thread::Builder::new()
            .name("obs-find".into())
            .spawn(work);
        #[cfg(target_arch = "wasm32")]
        work();
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
    fn stepped(&self, forward: bool, from: (usize, usize, usize)) -> Option<usize> {
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

    /// The first match at or after `(unit, row, byte)`, wrapping to the first of all.
    fn first_from(&self, (unit, row, byte): (usize, usize, usize)) -> usize {
        let i = self
            .matches
            .partition_point(|m| (m.unit, m.row, m.range.start) < (unit, row, byte));
        if i < self.matches.len() { i } else { 0 }
    }

    /// Find in a Resource Tab's text, then show the current match: the first at or after the
    /// top of the view when new matches arrive, the next or previous one on a step. A match
    /// out of view is scrolled to the top of a viewport `viewport` tall. True when the view
    /// moved (its Link set follows, as if the user had scrolled).
    pub(crate) fn update_reading(
        &mut self,
        view: &mut ReadingView,
        viewport: f32,
        ctx: &egui::Context,
    ) -> bool {
        let key = Arc::as_ptr(&view.doc) as usize as u64;
        let doc = Arc::clone(&view.doc);
        self.refresh(key, || FindSource::Doc(doc), ctx);
        let mut top = || {
            let at = view.text_anchor();
            at.map_or((0, 0, 0), |a| (a.unit, 0, a.byte))
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
        let m = &self.matches[i];
        let size = view.settings().size;
        let shown = view
            .text_top(m.unit, m.range.start, viewport)
            .is_some_and(|y| y >= 0.0 && y + 2.0 * size <= viewport);
        !shown && view.jump_to_text(m.unit, m.range.start)
    }

    /// [`Self::update_reading`] for a Text Comparison: `rows` are its texts, made again only
    /// when its Verse blocks (`generation`) changed. True when the view moved.
    pub(crate) fn update_comparison(
        &mut self,
        view: &mut crate::compare_view::ComparisonView,
        ctx: &egui::Context,
    ) -> bool {
        self.refresh(
            view.generation(),
            || FindSource::Rows(view.find_rows()),
            ctx,
        );
        let top = (view.top_block(), 0, 0);
        let arrived = self.poll();
        let show = if arrived && !self.matches.is_empty() {
            Some(self.first_from(top))
        } else if let Some(forward) = self.step.take() {
            self.stepped(forward, top)
        } else {
            None
        };
        let Some(i) = show else {
            return false;
        };
        self.current = Some(i);
        view.reveal_match(self.matches[i].unit)
    }
}
