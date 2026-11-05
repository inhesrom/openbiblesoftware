//! Re-anchoring (#65; ADR 0006; #17 rules 9–13; #20 rule 2): when a Resource changes, its
//! Annotations move with their text or become Orphaned annotations. None is ever deleted.
//!
//! - **When.** At library load ([`Session::reanchor_library`], from `attach_user`) and after
//!   an Import adds or replaces a Resource ([`Session::reanchor_resource`]), for each Resource
//!   whose content hash differs from the one stored on its anchors. The work runs as a
//!   background job; until it is applied the Resource is **re-anchoring**
//!   ([`Session::is_reanchoring`]) and reading panes do not draw its Marks, so nothing is
//!   ever drawn over the wrong words. A [`ReanchorReport`] says how many moved, fell back to
//!   whole Verses or were orphaned; a toast says so when anything did.
//! - **Matching** ([`locate`]), in order: follow the Resource's `unit_alias` table (a unit id
//!   renamed in the new revision); the exact quote at the stored offsets; the same quote
//!   elsewhere in the unit, picking the occurrence whose stored context fits best, then the
//!   nearest to the old position; the same comparison ignoring white space and punctuation.
//!   No approximate-string matching (excluded in v0.1).
//! - **Outcomes.** A Text-range anchor that is not found becomes **orphaned**: it keeps its
//!   quote, context and offsets and is listed under Needs attention. A Passage anchor never
//!   changes its Standard Passage; only its origin Text range is looked for, within the Verses
//!   of that Passage, and when it is not found the range is dropped, so the Annotation shows
//!   as whole Verses. Annotations of Resources not in the library are left alone (dormant)
//!   until the Resource comes back.
//! - **Writes** are one checked change list (`UserStore::apply_changes`), so a row edited
//!   meanwhile is never overwritten: the plan is made again. Automatic re-anchoring is not an
//!   undo step; the undo history is rebased onto the moved anchors, so undoing an earlier
//!   edit still lands.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;

use obs_model::{BibleVerse, UnitId};
use obs_scripture::Passage;
use obs_store::ResourceStore;
use obs_store::annotation::quote_with_context;
use serde_json::{Value, json};

use crate::annotations::{Anchor, Annotation, Change, Row, TextRange, standard_of};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::session::Session;
use crate::workspace::Effect;
use crate::{JobEvent, JobEventKind, JobId};

/// How often a plan that met rows changed underneath it is made again before giving up.
const RETRIES: u8 = 3;

/// What re-anchoring did to one Annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Found where it was (its content hash and context are brought up to date).
    Unchanged,
    /// Found elsewhere: other offsets, another unit (`unit_alias`), or an Orphaned annotation
    /// found again.
    Moved,
    /// A Passage anchor whose Text range was not found: the range is dropped and it shows as
    /// whole Verses.
    WholeVerses,
    /// A Text-range anchor that was not found: it is now an Orphaned annotation.
    Orphaned,
    /// An Orphaned annotation still not found.
    StillOrphaned,
}

/// What one re-anchoring run did in one Resource.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReanchorReport {
    pub resource_id: String,
    pub abbreviation: String,
    /// The content hash the Annotations were re-anchored to.
    pub content_hash: String,
    pub unchanged: usize,
    pub moved: usize,
    pub whole_verses: usize,
    pub orphaned: usize,
    pub still_orphaned: usize,
}

impl ReanchorReport {
    /// Annotations looked at.
    pub fn total(&self) -> usize {
        self.unchanged + self.moved + self.whole_verses + self.orphaned + self.still_orphaned
    }

    fn count(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Unchanged => self.unchanged += 1,
            Outcome::Moved => self.moved += 1,
            Outcome::WholeVerses => self.whole_verses += 1,
            Outcome::Orphaned => self.orphaned += 1,
            Outcome::StillOrphaned => self.still_orphaned += 1,
        }
    }

    /// The toast after a run, when anything moved, fell back or was orphaned: "WEB changed:
    /// 2 Annotations moved with their text, 1 needs attention (see Needs attention in
    /// Notes)."
    pub fn message(&self) -> Option<String> {
        let n = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let mut parts = Vec::new();
        if self.moved > 0 {
            parts.push(n(
                self.moved,
                "Annotation moved with its text",
                "Annotations moved with their text",
            ));
        }
        if self.whole_verses > 0 {
            parts.push(n(
                self.whole_verses,
                "now shows on whole Verses",
                "now show on whole Verses",
            ));
        }
        if self.orphaned > 0 {
            parts.push(format!(
                "{} (see Needs attention in Notes)",
                n(self.orphaned, "needs attention", "need attention")
            ));
        }
        (!parts.is_empty()).then(|| format!("{} changed: {}.", self.abbreviation, parts.join(", ")))
    }
}

/// One Annotation's re-anchoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub before: Annotation,
    pub after: Annotation,
    pub outcome: Outcome,
}

/// The re-anchoring of one Resource's Annotations against its current revision, made off the
/// UI thread and applied by the Session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub report: ReanchorReport,
    pub items: Vec<Planned>,
}

impl Plan {
    /// The row updates to write: one per Annotation that changed. Never an insert or a
    /// delete: re-anchoring keeps every Annotation.
    pub fn changes(&self) -> Vec<Change> {
        self.items
            .iter()
            .filter(|p| p.after != p.before)
            .map(|p| {
                Change::update(
                    Row::Annotation(p.before.clone()),
                    Row::Annotation(p.after.clone()),
                )
            })
            .collect()
    }
}

/// Whether an Annotation's Text range in Resource `resource_id` (current content hash
/// `hash`) needs re-anchoring: it was made against another revision; or it is orphaned and
/// the Resource is back at the revision it was made against.
pub fn is_stale(a: &Annotation, resource_id: &str, hash: &str) -> bool {
    a.anchor
        .as_ref()
        .and_then(Anchor::text_range)
        .is_some_and(|r| r.resource_id == resource_id && (r.content_hash != hash) != r.orphaned)
}

/// Whether an Annotation has a Text range in Resource `resource_id`.
fn refers_to(a: &Annotation, resource_id: &str) -> bool {
    a.anchor
        .as_ref()
        .and_then(Anchor::text_range)
        .is_some_and(|r| r.resource_id == resource_id)
}

/// Letters and digits: what the white-space- and punctuation-insensitive match compares.
fn significant(c: char) -> bool {
    c.is_alphanumeric()
}

/// Punctuation or a symbol: neither a letter or digit nor white space.
fn mark(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace()
}

/// How well an occurrence's surroundings fit the stored context: characters of `prefix`
/// matching backwards from the occurrence plus characters of `suffix` matching forwards.
fn context_fit(
    prefix: impl DoubleEndedIterator<Item = char>,
    before: impl DoubleEndedIterator<Item = char>,
    suffix: impl Iterator<Item = char>,
    after: impl Iterator<Item = char>,
) -> usize {
    let back = prefix
        .rev()
        .zip(before.rev())
        .take_while(|(a, b)| a == b)
        .count();
    let forward = suffix.zip(after).take_while(|(a, b)| a == b).count();
    back + forward
}

/// The best of the candidates: the best context fit, then the nearest to `near`, then the
/// first.
fn best(candidates: Vec<(usize, Range<usize>)>, near: usize) -> Option<Range<usize>> {
    candidates
        .into_iter()
        .min_by_key(|(fit, r)| (std::cmp::Reverse(*fit), r.start.abs_diff(near), r.start))
        .map(|(_, r)| r)
}

/// Where `range`'s quote is now in `text` (its unit's text in the new revision), looking only
/// inside `window` (byte offsets on character boundaries; the whole unit for a Text-range
/// anchor, the Passage's Verses for a Passage anchor's Text range). In order: the exact quote
/// at the stored offsets; the exact quote elsewhere (best context, then nearest); the same
/// ignoring white space and punctuation. `None`: not found.
pub fn locate(range: &TextRange, text: &str, window: Range<usize>) -> Option<Range<usize>> {
    let quote = range.quote.as_str();
    let window = window.start.min(text.len())..window.end.min(text.len());
    let hay = text.get(window.clone())?;
    if quote.is_empty() || hay.is_empty() {
        return None;
    }
    let (s, e) = (range.start as usize, range.end as usize);
    if s >= window.start && e <= window.end && text.get(s..e) == Some(quote) {
        return Some(s..e);
    }
    // The same quote elsewhere in the window (overlapping occurrences too).
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = hay.get(from..).and_then(|h| h.find(quote)) {
        let a = window.start + from + i;
        let b = a + quote.len();
        let fit = context_fit(
            range.prefix.chars(),
            text.get(..a).unwrap_or_default().chars(),
            range.suffix.chars(),
            text.get(b..).unwrap_or_default().chars(),
        );
        found.push((fit, a..b));
        from += i + hay[from + i..].chars().next().map_or(1, char::len_utf8);
    }
    if let Some(r) = best(found, s) {
        return Some(r);
    }
    loose(range, text, window)
}

/// The white-space- and punctuation-insensitive match: the quote's letters and digits as a
/// run of the window's letters and digits. The match then takes in punctuation next to it as
/// far as the quote had punctuation at that end ("wept." finds "wept ." whole).
fn loose(range: &TextRange, text: &str, window: Range<usize>) -> Option<Range<usize>> {
    let quote: Vec<char> = range.quote.chars().filter(|c| significant(*c)).collect();
    let first = *quote.first()?;
    let hay = text.get(window.clone())?;
    // The window's letters and digits with their byte ranges in `text`.
    let chars: Vec<(char, usize)> = hay
        .char_indices()
        .filter(|(_, c)| significant(*c))
        .map(|(i, c)| (c, window.start + i))
        .collect();
    if chars.len() < quote.len() {
        return None;
    }
    let prefix: Vec<char> = range.prefix.chars().filter(|c| significant(*c)).collect();
    let suffix: Vec<char> = range.suffix.chars().filter(|c| significant(*c)).collect();
    let mut found = Vec::new();
    for k in 0..=chars.len() - quote.len() {
        if chars[k].0 != first
            || !chars[k..k + quote.len()]
                .iter()
                .map(|(c, _)| *c)
                .eq(quote.iter().copied())
        {
            continue;
        }
        let (last, at) = chars[k + quote.len() - 1];
        let fit = context_fit(
            prefix.iter().copied(),
            chars[..k].iter().map(|(c, _)| *c),
            suffix.iter().copied(),
            chars[k + quote.len()..].iter().map(|(c, _)| *c),
        );
        found.push((fit, chars[k].1..at + last.len_utf8()));
    }
    let mut r = best(found, range.start as usize)?;
    // Take in as much punctuation at each end as the quote had there, across white space.
    let lead = range.quote.chars().take_while(|c| mark(*c)).count();
    let trail = range.quote.chars().rev().take_while(|c| mark(*c)).count();
    let (mut taken, mut at) = (0, r.start);
    for c in text[window.start..r.start].chars().rev() {
        if taken == lead || !(mark(c) || c.is_whitespace()) {
            break;
        }
        at -= c.len_utf8();
        if mark(c) {
            taken += 1;
            r.start = at;
        }
    }
    let (mut taken, mut at) = (0, r.end);
    for c in text[r.end..window.end].chars() {
        if taken == trail || !(mark(c) || c.is_whitespace()) {
            break;
        }
        at += c.len_utf8();
        if mark(c) {
            taken += 1;
            r.end = at;
        }
    }
    Some(r)
}

/// A Bible's Verses with their Standard Passages, read once per plan.
struct Verses(Vec<(BibleVerse, Option<Passage>)>);

impl Verses {
    fn read(store: &dyn ResourceStore) -> Result<Self, String> {
        let map = store.verse_map().map_err(|e| e.to_string())?;
        let verses = store.verses().map_err(|e| e.to_string())?;
        Ok(Self(
            verses
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let mapped = map
                        .as_ref()
                        .map(|m| m.entries().get(i).and_then(|e| e.standard));
                    let standard = standard_of(mapped, v.verse, v.last);
                    (v, standard)
                })
                .collect(),
        ))
    }

    /// The bytes of `unit` its Verses of `passage` cover, first to last.
    fn window(&self, unit: &UnitId, passage: Passage) -> Option<Range<usize>> {
        let mut out: Option<Range<usize>> = None;
        for (v, standard) in &self.0 {
            if v.unit != *unit || !standard.is_some_and(|p| p.overlaps(passage)) {
                continue;
            }
            let r = v.range();
            out = Some(match out {
                Some(o) => o.start.min(r.start)..o.end.max(r.end),
                None => r,
            });
        }
        out
    }
}

/// Re-anchor the Annotations that have a Text range in `store`'s Resource against its
/// current revision. Annotations of other Resources are left out. Pure: nothing is written.
pub fn plan(store: &dyn ResourceStore, annotations: &[Annotation]) -> Result<Plan, String> {
    let manifest = store.manifest();
    let rid = manifest.resource_id.as_str();
    let hash = manifest.content_hash.as_str();
    let mut report = ReanchorReport {
        resource_id: rid.to_string(),
        abbreviation: manifest.abbreviation.clone(),
        content_hash: hash.to_string(),
        ..ReanchorReport::default()
    };
    let mut texts: HashMap<UnitId, Option<String>> = HashMap::new();
    let mut verses: Option<Verses> = None;
    let mut items = Vec::new();
    for a in annotations.iter().filter(|a| refers_to(a, rid)) {
        let Some(anchor) = a.anchor.as_ref() else {
            continue;
        };
        let Some(range) = anchor.text_range() else {
            continue;
        };
        // A unit id renamed in this revision: the alias says where it lives now.
        let unit = store
            .unit_alias(&range.unit)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| range.unit.clone());
        if !texts.contains_key(&unit) {
            let text = store
                .unit_text(&unit)
                .map_err(|e| e.to_string())?
                .map(|(_, t)| t);
            texts.insert(unit.clone(), text);
        }
        let text = texts.get(&unit).and_then(Option::as_deref);
        let window = match (anchor, text) {
            (_, None) => None,
            (Anchor::Passage { passage, .. }, Some(_)) => {
                if verses.is_none() {
                    verses = Some(Verses::read(store)?);
                }
                verses.as_ref().and_then(|v| v.window(&unit, *passage))
            }
            (Anchor::TextRange { .. }, Some(t)) => Some(0..t.len()),
        };
        let found = text.zip(window).and_then(|(t, w)| {
            let r = locate(range, t, w)?;
            let (prefix, quote, suffix) = quote_with_context(t, r.start, r.end)?;
            Some(TextRange {
                unit: unit.clone(),
                start: u32::try_from(r.start).ok()?,
                end: u32::try_from(r.end).ok()?,
                quote: quote.to_string(),
                prefix: prefix.to_string(),
                suffix: suffix.to_string(),
                content_hash: hash.to_string(),
                orphaned: false,
                ..range.clone()
            })
        });
        let (anchor, outcome) = match (anchor, found) {
            (Anchor::Passage { passage, .. }, Some(origin)) => {
                let moved = origin.unit != range.unit
                    || (origin.start, origin.end) != (range.start, range.end);
                (
                    Anchor::Passage {
                        passage: *passage,
                        origin: Some(origin),
                    },
                    if moved {
                        Outcome::Moved
                    } else {
                        Outcome::Unchanged
                    },
                )
            }
            (Anchor::TextRange { .. }, Some(found)) => {
                let moved = range.orphaned
                    || found.unit != range.unit
                    || (found.start, found.end) != (range.start, range.end);
                (
                    Anchor::TextRange { range: found },
                    if moved {
                        Outcome::Moved
                    } else {
                        Outcome::Unchanged
                    },
                )
            }
            // The Standard Passage stays; the Highlight or Note shows as whole Verses.
            (Anchor::Passage { passage, .. }, None) => (
                Anchor::Passage {
                    passage: *passage,
                    origin: None,
                },
                Outcome::WholeVerses,
            ),
            // Kept with what it last pointed at, until the user re-attaches, detaches or
            // deletes it.
            (Anchor::TextRange { .. }, None) => (
                Anchor::TextRange {
                    range: TextRange {
                        orphaned: true,
                        ..range.clone()
                    },
                },
                if range.orphaned {
                    Outcome::StillOrphaned
                } else {
                    Outcome::Orphaned
                },
            ),
        };
        report.count(outcome);
        items.push(Planned {
            before: a.clone(),
            after: Annotation {
                anchor: Some(anchor),
                ..a.clone()
            },
            outcome,
        });
    }
    Ok(Plan { report, items })
}

/// Re-anchoring jobs in flight and the last report of each Resource.
#[derive(Debug, Default)]
pub struct Reanchoring {
    /// Job → the Resource it re-anchors.
    running: BTreeMap<JobId, String>,
    /// Resources asked again while their job ran: checked again when it finishes.
    again: BTreeSet<String>,
    /// Plans made again because rows changed underneath them.
    retries: BTreeMap<String, u8>,
    reports: BTreeMap<String, ReanchorReport>,
}

impl Reanchoring {
    /// Whether any Resource is being re-anchored.
    pub fn is_busy(&self) -> bool {
        !self.running.is_empty()
    }

    /// The last run's report for a Resource.
    pub fn report(&self, resource_id: &str) -> Option<&ReanchorReport> {
        self.reports.get(resource_id)
    }

    fn is_running(&self, resource_id: &str) -> bool {
        self.running.values().any(|r| r == resource_id)
    }
}

impl Session {
    /// Whether a Resource's Annotations are being re-anchored. Until they are, reading panes
    /// do not draw its Marks.
    pub fn is_reanchoring(&self, resource_id: &str) -> bool {
        self.reanchoring.is_running(resource_id)
    }

    /// Check every library Resource and re-anchor, in the background, those with Annotations
    /// made against another revision. Returns the Resources queued.
    pub fn reanchor_library(&mut self) -> Vec<String> {
        let Some(all) = self.user.as_ref().and_then(|u| u.annotations(None).ok()) else {
            return Vec::new();
        };
        let ids: Vec<String> = self
            .library
            .entries()
            .iter()
            .filter(|e| {
                all.iter()
                    .any(|a| is_stale(a, &e.manifest.resource_id, &e.manifest.content_hash))
            })
            .map(|e| e.manifest.resource_id.clone())
            .collect();
        for id in &ids {
            self.submit_reanchor(id, &all);
        }
        ids
    }

    /// [`Self::reanchor_library`] for one Resource (after an Import added or replaced it).
    /// Returns whether a job was queued (or will run again).
    pub fn reanchor_resource(&mut self, resource_id: &str) -> bool {
        let Some(entry) = self.library.get(resource_id) else {
            return false;
        };
        let hash = entry.manifest.content_hash.clone();
        let Some(all) = self.user.as_ref().and_then(|u| u.annotations(None).ok()) else {
            return false;
        };
        if !all.iter().any(|a| is_stale(a, resource_id, &hash)) {
            return false;
        }
        self.submit_reanchor(resource_id, &all);
        true
    }

    fn submit_reanchor(&mut self, resource_id: &str, all: &[Annotation]) {
        if self.reanchoring.is_running(resource_id) {
            self.reanchoring.again.insert(resource_id.to_string());
            return;
        }
        let Some(entry) = self.library.get(resource_id) else {
            return;
        };
        // Every Annotation of the Resource: those made against older revisions, and the
        // Orphaned ones, which may be found in this one.
        let mine: Vec<Annotation> = all
            .iter()
            .filter(|a| refers_to(a, resource_id))
            .cloned()
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = entry.path.clone();
            let job = self
                .jobs()
                .submit_fn(format!("Re-anchor {resource_id}"), move |_ctx| {
                    let reader = obs_store::ObrReader::open(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    plan(&reader, &mine)
                });
            self.reanchoring
                .running
                .insert(job, resource_id.to_string());
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Resources cannot be opened here yet; nothing is shown that could be wrong.
            let _ = (entry, mine);
        }
    }

    /// Apply re-anchoring job events; return the others.
    pub(crate) fn poll_reanchoring(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            let Some(id) = self.reanchoring.running.get(&event.id).cloned() else {
                other.push(event);
                continue;
            };
            if !event.kind.is_terminal() {
                continue;
            }
            self.reanchoring.running.remove(&event.id);
            match event.kind {
                JobEventKind::Finished(out) => {
                    if let Ok(plan) = out.downcast::<Plan>() {
                        self.apply_plan(*plan);
                    }
                }
                JobEventKind::Failed(e) => {
                    self.workspace.push_effect(Effect::Message(format!(
                        "Could not re-anchor the Annotations in {id}: {e}"
                    )));
                }
                _ => {}
            }
            if self.reanchoring.again.remove(&id) {
                self.reanchor_resource(&id);
            }
        }
        other
    }

    /// Write a finished plan as one checked change list. If the Resource changed again or a
    /// row changed meanwhile, plan again instead.
    fn apply_plan(&mut self, plan: Plan) {
        let id = plan.report.resource_id.clone();
        let current = self
            .library
            .get(&id)
            .map(|e| e.manifest.content_hash.clone());
        match current {
            // Removed meanwhile: its Annotations stay as they are (dormant).
            None => return,
            Some(hash) if hash != plan.report.content_hash => {
                self.reanchor_resource(&id);
                return;
            }
            Some(_) => {}
        }
        let changes = plan.changes();
        if !changes.is_empty() {
            let Some(user) = self.user.as_deref() else {
                return;
            };
            if let Err(e) = user.apply_changes(&changes) {
                let tries = self.reanchoring.retries.entry(id.clone()).or_insert(0);
                *tries += 1;
                if *tries <= RETRIES {
                    self.reanchor_resource(&id);
                } else {
                    self.reanchoring.retries.remove(&id);
                    self.workspace.push_effect(Effect::Message(format!(
                        "Could not re-anchor the Annotations in {}: {e}",
                        plan.report.abbreviation
                    )));
                }
                return;
            }
            for p in plan.items.iter().filter(|p| p.after != p.before) {
                self.history.rebase(&p.before, &p.after);
            }
            self.annotations_revision += 1;
        }
        self.reanchoring.retries.remove(&id);
        if let Some(m) = plan.report.message() {
            self.workspace.push_effect(Effect::Message(m));
        }
        self.reanchoring.reports.insert(id, plan.report);
    }

    /// The Orphaned annotations (Needs attention), oldest first.
    pub fn orphaned_annotations(&self) -> Result<Vec<Annotation>, String> {
        match &self.user {
            Some(u) => Ok(u
                .annotations(None)?
                .into_iter()
                .filter(Annotation::is_orphaned)
                .collect()),
            None => Ok(Vec::new()),
        }
    }
}

// -------------------------------------------------------------------------------- commands

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "annotation.reanchor",
    label: "Re-anchor Annotations",
    shortcut: None,
    params: &[ParamSpec {
        name: "resource",
        kind: ParamKind::String,
        required: false,
        help: "Only this Resource (default: every Resource in the library)",
    }],
    enabled: has_user,
    run: run_reanchor,
}];

fn run_reanchor(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let queued = match p.str("resource") {
        Some(id) => {
            if s.library.get(id).is_none() {
                return Err(CommandError::Failed {
                    command: "annotation.reanchor",
                    message: format!("{id} is not in the library"),
                });
            }
            if s.reanchor_resource(id) {
                vec![id.to_string()]
            } else {
                Vec::new()
            }
        }
        None => s.reanchor_library(),
    };
    Ok(json!({ "queued": queued }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn range(text: &str, quote: &str, nth: usize) -> TextRange {
        let start = text.match_indices(quote).nth(nth).unwrap().0;
        let (prefix, quote, suffix) = quote_with_context(text, start, start + quote.len()).unwrap();
        TextRange {
            resource_id: "r".into(),
            unit: UnitId::new("U").unwrap(),
            start: start as u32,
            end: (start + quote.len()) as u32,
            quote: quote.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
            content_hash: "sha256:old".into(),
            native_ref: None,
            whole_verses: false,
            orphaned: false,
        }
    }

    fn at(text: &str, r: Option<Range<usize>>) -> Option<&str> {
        r.map(|r| &text[r])
    }

    #[test]
    fn the_quote_at_its_offsets_stays() {
        let t = "In the beginning God created the heaven and the earth.";
        let r = range(t, "God created", 0);
        assert_eq!(
            locate(&r, t, 0..t.len()),
            Some(r.start as usize..r.end as usize)
        );
    }

    #[test]
    fn a_repeated_quote_goes_to_the_occurrence_whose_context_fits() {
        let old = "the LORD is my shepherd. Surely the LORD is good. And the LORD is near.";
        let r = range(old, "the LORD", 1);
        // A sentence inserted before: all three move; context picks the second again.
        let new = format!("Praise ye the LORD. {old}");
        let found = locate(&r, &new, 0..new.len()).unwrap();
        assert_eq!(found.start, r.start as usize + "Praise ye the LORD. ".len());
        assert_eq!(&new[found.clone()], "the LORD");
        assert!(new[..found.start].ends_with("Surely "));
    }

    #[test]
    fn equal_context_picks_the_nearest_occurrence() {
        let r = TextRange {
            prefix: String::new(),
            suffix: String::new(),
            start: 30,
            end: 33,
            ..range("abc", "abc", 0)
        };
        let t = "xx abc xxxxxxxxxxxxxxxxxxxxxxxx abc xxxxxxxxxxxxxxxxxxxxxxx abc";
        let found = locate(&r, t, 0..t.len()).unwrap();
        assert_eq!(found.start, t.match_indices("abc").nth(1).unwrap().0);
    }

    #[test]
    fn white_space_and_punctuation_are_ignored_last() {
        let old = "Jesus wept. Then said the Jews, Behold how he loved him!";
        let r = range(old, "Then said the Jews,", 0);
        let new = "Jesus wept.  Then said the Jews ,Behold how he loved him!";
        assert_eq!(
            at(new, locate(&r, new, 0..new.len())),
            Some("Then said the Jews ,")
        );
        // Letters changed: not found (no fuzzy matching).
        let typo = "Jesus wept. Than said the Jews, Behold how he loved him!";
        assert_eq!(locate(&r, typo, 0..typo.len()), None);
    }

    #[test]
    fn the_window_limits_the_search() {
        let t = "grace and peace. grace and peace.";
        let r = TextRange {
            start: 100,
            end: 105,
            ..range(t, "grace", 0)
        };
        let second = t.rfind("grace").unwrap();
        assert_eq!(
            locate(&r, t, second..t.len()).map(|r| r.start),
            Some(second)
        );
        assert_eq!(locate(&r, t, 5..15), None);
    }

    #[test]
    fn stale_means_another_revision_or_an_orphan_back_at_its_own() {
        let mut a = Annotation {
            id: "a".into(),
            notebook_id: "n".into(),
            content: crate::Content::Note { body: "x".into() },
            anchor: Some(Anchor::TextRange {
                range: range("ab", "ab", 0),
            }),
            created_at: 0,
            modified_at: 0,
        };
        assert!(is_stale(&a, "r", "sha256:new"));
        assert!(!is_stale(&a, "r", "sha256:old"));
        assert!(!is_stale(&a, "other", "sha256:new"));
        if let Some(Anchor::TextRange { range }) = a.anchor.as_mut() {
            range.orphaned = true;
        }
        assert!(!is_stale(&a, "r", "sha256:new"));
        assert!(is_stale(&a, "r", "sha256:old"));
    }
}
