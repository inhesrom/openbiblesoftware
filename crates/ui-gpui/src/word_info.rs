//! Word Info from the reading pane (#381; #18 rules 15–16): the Tagged word the pointer rests
//! on for [`HOVER_DWELL`] is shown in the engine's Word Info (sticky: moving off every word
//! keeps it), and a click pins it, as the egui Shell's `word_hovered` does. The Word Info Tab
//! that draws it comes with #390; here the Session's [`obs_engine::WordInfo`] is fed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{Context, Task};
use obs_engine::word_info::lexicon_gloss;
use obs_engine::{ShownWord, TabId};
use obs_model::ResourceType;
use obs_reading::ReadingDoc;
use obs_scripture::{Passage, RefStyle, format_passage};
use serde_json::json;

use crate::Shell;
use crate::reading::WordHit;

/// How long the pointer rests on a Tagged word before Word Info follows it (the egui Shell's
/// `HOVER_DWELL_SECS`).
pub const HOVER_DWELL: Duration = Duration::from_millis(150);

/// The Tagged word the pointer is resting on, for Word Info's dwell.
pub(crate) struct Dwell {
    tab: TabId,
    word: WordHit,
    since: Instant,
    /// Word Info shows it already.
    shown: bool,
    /// Wakes the Shell when the dwell is over.
    _timer: Option<Task<()>>,
}

/// What Word Info shows for Tagged word `hit` of `doc` (the egui Shell's `shown_word`).
pub fn shown_word(
    session: &obs_engine::Session,
    doc: &ReadingDoc,
    hit: &WordHit,
) -> Option<ShownWord> {
    let (range, parts) = doc.tagged_word_at(hit.unit, hit.range.start)?;
    let unit = doc.units.get(hit.unit)?;
    let written = unit.text.get(range.clone())?.trim();
    let reference = match doc.verse_containing(hit.unit, range.start) {
        Some(v) => format!(
            "{} ({})",
            format_passage(Passage::verse(v), &doc.versification, RefStyle::Long),
            doc.manifest.abbreviation
        ),
        None => format!("({})", doc.manifest.abbreviation),
    };
    let rtl = doc.direction() == obs_model::Direction::Rtl;
    let library = &session.library;
    let mut shown = ShownWord::new(written, rtl, reference, parts, |n| {
        lexicon_gloss(library, n)
    })?;
    shown.bible = Some(doc.manifest.resource_id.clone());
    Some(shown)
}

impl Shell {
    /// A reading pane reported what the pointer is on: start or continue the dwell, show the
    /// word once it has rested long enough, and pin it when clicked. A click on a Strong's
    /// link outside a Bible opens its Word Study instead (#55).
    pub(crate) fn word_hovered(
        &mut self,
        tab: TabId,
        doc: &Arc<ReadingDoc>,
        hovered: Option<WordHit>,
        clicked: Option<(WordHit, bool)>,
        cx: &mut Context<Self>,
    ) {
        let now = cx.background_executor().now();
        if let Some((w, new_tab)) = clicked {
            if self.open_strongs_link(tab, doc, &w, new_tab) {
                return;
            }
            if let Some(word) = shown_word(&self.session, doc, &w) {
                self.session.word_info.pin(word);
                self.dwell = Some(Dwell {
                    tab,
                    word: w,
                    since: now,
                    shown: true,
                    _timer: None,
                });
                return;
            }
        }
        let Some(w) = hovered else {
            // Off every word: Word Info keeps what it last showed (sticky).
            if self.dwell.as_ref().is_some_and(|d| d.tab == tab) {
                self.dwell = None;
            }
            return;
        };
        let same = self
            .dwell
            .as_ref()
            .is_some_and(|d| d.tab == tab && d.word == w);
        if !same {
            // Wake when the dwell is over, so Word Info follows a pointer that rests.
            let timer = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HOVER_DWELL).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            self.dwell = Some(Dwell {
                tab,
                word: w,
                since: now,
                shown: false,
                _timer: Some(timer),
            });
        }
        let Some(d) = self.dwell.as_mut() else {
            return;
        };
        if d.shown || now.saturating_duration_since(d.since) < HOVER_DWELL {
            return;
        }
        d.shown = true;
        let word = d.word.clone();
        if let Some(shown) = shown_word(&self.session, doc, &word) {
            self.session.word_info.hover(shown);
        }
    }

    /// A click on a Strong's link in a Lexicon, Dictionary or Commentary (a Tagged word
    /// outside a Bible) opens its Word Study; Ctrl/Cmd-click opens a new one. Returns whether
    /// it did.
    fn open_strongs_link(
        &mut self,
        tab: TabId,
        doc: &ReadingDoc,
        hit: &WordHit,
        new_tab: bool,
    ) -> bool {
        if doc.manifest.resource_type == ResourceType::Bible {
            return false;
        }
        let Some(number) = doc
            .tagged_word_at(hit.unit, hit.range.start)
            .and_then(|(_, parts)| parts.first().map(|p| p.strongs))
        else {
            return false;
        };
        self.session.workspace.user_focused(tab);
        self.run(
            "word_study.open",
            &json!({ "strongs": number.to_string(), "new_tab": new_tab }),
        );
        true
    }
}
