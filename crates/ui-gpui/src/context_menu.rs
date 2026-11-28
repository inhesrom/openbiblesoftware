//! The right-click menu of a reading pane (#142, on GPUI #381): two columns, as in the egui
//! app. The left column lists what the click can act on (the selected text, the Verse, and
//! for a Tagged word its Lemma, Strong's number and Morphology); the right column the actions
//! for the one chosen (hovering a target chooses it).
//!
//! | Target | Actions |
//! |---|---|
//! | Selection | Copy, Copy verses with formatting, Search this, Look up in Dictionary, Word Study… |
//! | Verse | Passage Guide, Exegetical Guide, Cross-references, Text Comparison, Copy with reference |
//! | Lemma, Strong's number | Word Study, Open in preferred Lexicon, Pin in Word Info, Search Bible for it |
//! | Morphology | Copy, Pin in Word Info |
//!
//! Actions run engine commands (`word_study.open`, `search.run`, …); with Ctrl/Cmd held they
//! open a new Tab. Shift+F10 opens it from the keyboard. The Highlight styles, Note… and Add
//! Note on verse come with Highlights and Notes (#382). Copy verses with formatting copies the
//! Verses the selection reaches through the engine's `copy_verses` (#154), with the Copy verses
//! options in use.

use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, SharedString, Window, div, px,
};
use obs_engine::link::{ToStandard, to_standard};
use obs_engine::{CopiedBible, TabId};
use obs_model::ResourceType;
use obs_reading::{ReadingDoc, copy_text, selected_text};
use obs_scripture::{
    Passage, PassageList, RefStyle, Standard, StrongsNumber, Verse, format_passage,
};
use serde_json::json;

use crate::Shell;
use crate::components;
use crate::reading::{MenuRequest, ReadingPane, WordHit, has_text, verse_selection};
use crate::theme::Look;
use crate::tokens::{self, space};

/// One thing the menu can act on: a row of its left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Selection,
    Verse,
    Lemma,
    Strongs,
    Morphology,
}

impl Target {
    /// What the left column calls it, above its value.
    pub fn kind(self) -> &'static str {
        match self {
            Target::Selection => "Selection",
            Target::Verse => "Verse",
            Target::Lemma => "Lemma",
            Target::Strongs => "Strong's number",
            Target::Morphology => "Morphology",
        }
    }
}

/// The Verse the menu was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseTarget {
    /// "Jn 3:16", in the numbering of where it was clicked.
    pub label: String,
    /// In the Standard versification; `None` for an Unmapped verse.
    pub standard: Option<Verse>,
    /// In a Bible's reading pane: the Bible's own Verse (Copy with reference copies it).
    pub native: Option<Verse>,
}

/// The Tagged word the menu was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordTarget {
    pub hit: WordHit,
    pub strongs: StrongsNumber,
    pub lemma: Option<String>,
    /// The Bible tags the word with its Lemma (an English Bible's Lemma comes from a
    /// Lexicon, and its text cannot be searched by Lemma: it is searched by number instead).
    pub tagged_lemma: bool,
    /// Plain English where the code is one we read, else the code as tagged.
    pub morphology: Option<String>,
}

/// Everything the menu offers, gathered when it opens and kept while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuTargets {
    /// The selected text, as selected.
    pub selection: Option<String>,
    /// The Standard Verses the selection reaches, for Copy verses with formatting.
    pub selected_verses: Option<PassageList>,
    pub verse: Option<VerseTarget>,
    pub word: Option<WordTarget>,
    /// The Resource clicked in (id and type), for Search this.
    pub resource: Option<(String, ResourceType)>,
    /// The target whose actions the right column shows.
    pub chosen: Target,
}

impl MenuTargets {
    fn new(
        selection: Option<String>,
        selected_verses: Option<PassageList>,
        verse: Option<VerseTarget>,
        word: Option<WordTarget>,
        resource: Option<(String, ResourceType)>,
    ) -> Self {
        let mut m = Self {
            selection,
            selected_verses,
            verse,
            word,
            resource,
            chosen: Target::Selection,
        };
        // A Tagged word's study comes first; then the selection; then the Verse.
        let targets = m.targets();
        m.chosen = [Target::Lemma, Target::Strongs, Target::Selection]
            .into_iter()
            .find(|t| targets.contains(t))
            .or_else(|| targets.first().copied())
            .unwrap_or(Target::Selection);
        m
    }

    /// The left column, in order.
    pub fn targets(&self) -> Vec<Target> {
        let mut out = Vec::new();
        if self.selection.is_some() {
            out.push(Target::Selection);
        }
        if self.verse.is_some() {
            out.push(Target::Verse);
        }
        if let Some(w) = &self.word {
            if w.lemma.is_some() {
                out.push(Target::Lemma);
            }
            out.push(Target::Strongs);
            if w.morphology.is_some() {
                out.push(Target::Morphology);
            }
        }
        out
    }

    /// A target's value as the left column shows it.
    pub fn value(&self, target: Target) -> String {
        match target {
            Target::Selection => {
                let text = self.selection.as_deref().unwrap_or_default();
                let words: Vec<&str> = text.split_whitespace().collect();
                let mut shown = words.join(" ");
                if shown.chars().count() > 28 {
                    shown = shown.chars().take(27).collect::<String>() + "…";
                }
                format!("“{shown}”")
            }
            Target::Verse => self
                .verse
                .as_ref()
                .map(|v| v.label.clone())
                .unwrap_or_default(),
            Target::Lemma => self
                .word
                .as_ref()
                .and_then(|w| w.lemma.clone())
                .unwrap_or_default(),
            Target::Strongs => self
                .word
                .as_ref()
                .map(|w| w.strongs.to_string())
                .unwrap_or_default(),
            Target::Morphology => self
                .word
                .as_ref()
                .and_then(|w| w.morphology.clone())
                .unwrap_or_default(),
        }
    }

    /// The selection is a word or short phrase: the English-word chooser can take it.
    fn short_selection(&self) -> Option<&str> {
        let text = self.selection.as_deref()?.trim();
        let words = text.split_whitespace().count();
        (1..=3).contains(&words).then_some(text)
    }

    /// The chosen target's actions, in order: each with its label and whether it can run now.
    pub fn actions(&self) -> Vec<(MenuAction, String, bool)> {
        let mut out = Vec::new();
        match self.chosen {
            Target::Selection => {
                out.push((MenuAction::Copy, "Copy".into(), true));
                out.push((
                    MenuAction::CopyVerses,
                    "Copy verses with formatting".into(),
                    self.selected_verses.is_some(),
                ));
                out.push((MenuAction::SearchThis, "Search this".into(), true));
                out.push((MenuAction::LookUp, "Look up in Dictionary".into(), true));
                out.push((
                    MenuAction::ChooseWord,
                    "Word Study…".into(),
                    self.short_selection().is_some(),
                ));
            }
            Target::Verse => {
                let Some(v) = &self.verse else {
                    return out;
                };
                let mapped = v.standard.is_some();
                for (action, label) in [
                    (MenuAction::PassageGuide, "Passage Guide"),
                    (MenuAction::ExegeticalGuide, "Exegetical Guide"),
                    (MenuAction::CrossReferences, "Cross-references"),
                    (MenuAction::TextComparison, "Text Comparison"),
                ] {
                    out.push((action, label.into(), mapped));
                }
                out.push((
                    MenuAction::CopyVerse,
                    "Copy with reference".into(),
                    v.native.is_some(),
                ));
            }
            Target::Lemma | Target::Strongs => {
                let Some(w) = &self.word else {
                    return out;
                };
                out.push((MenuAction::WordStudy, "Word Study".into(), true));
                out.push((
                    MenuAction::OpenLexicon,
                    "Open in preferred Lexicon".into(),
                    true,
                ));
                out.push((MenuAction::PinWordInfo, "Pin in Word Info".into(), true));
                let (shown, action) = match (self.chosen, &w.lemma) {
                    (Target::Lemma, Some(lemma)) => (lemma.clone(), MenuAction::SearchLemma),
                    _ => (w.strongs.to_string(), MenuAction::SearchStrongs),
                };
                out.push((action, format!("Search Bible for {shown}"), true));
            }
            Target::Morphology => {
                out.push((MenuAction::CopyMorphology, "Copy".into(), true));
                out.push((MenuAction::PinWordInfo, "Pin in Word Info".into(), true));
            }
        }
        out
    }
}

/// Something done from the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Copy,
    CopyVerses,
    SearchThis,
    LookUp,
    ChooseWord,
    PassageGuide,
    ExegeticalGuide,
    CrossReferences,
    TextComparison,
    CopyVerse,
    WordStudy,
    OpenLexicon,
    PinWordInfo,
    SearchLemma,
    SearchStrongs,
    CopyMorphology,
}

/// An open right-click menu.
pub struct ContextMenu {
    pub tab: TabId,
    pub targets: MenuTargets,
    /// Where it opens, in window points.
    pub at: Point<Pixels>,
    focus: FocusHandle,
    /// The keyboard was put in it.
    focused: bool,
}

/// "Jn 3:16" in `doc`'s own numbering.
fn short_label(doc: &ReadingDoc, verse: Verse) -> String {
    format_passage(Passage::verse(verse), &doc.versification, RefStyle::Short)
}

/// The Verses (Standard) a selection reaches in a Bible, for Copy verses with formatting.
fn selected_verses(doc: &ReadingDoc, pane: &ReadingPane) -> Option<PassageList> {
    if doc.manifest.resource_type != ResourceType::Bible {
        return None;
    }
    let sel = pane.selection().filter(|s| has_text(pane.view(), s))?;
    let (a, b) = sel.range(doc);
    let first = doc.verse_containing(a.unit, a.byte)?;
    let last = doc
        .verse_containing(b.unit, b.byte.saturating_sub(1).max(a.byte))
        .unwrap_or(first);
    let standard = |v: Verse| match to_standard(doc.verse_map.as_deref(), v) {
        ToStandard::Verse(s) => Some(s),
        ToStandard::Unmapped => None,
    };
    let (first, last) = (standard(first)?, standard(last.max(first))?);
    let passage = Passage::new(first, last.max(first)).unwrap_or(Passage::verse(first));
    Some(vec![passage].into())
}

impl Shell {
    /// What a reading pane's menu offers (the egui Shell's `reading_targets`): its selection
    /// (the word right-clicked, or the selection it was in), the Verse there, and the Tagged
    /// word there.
    fn reading_targets(&self, pane: &ReadingPane, req: &MenuRequest) -> MenuTargets {
        let view = pane.view();
        let doc = Arc::clone(&view.doc);
        let sel = pane.selection().filter(|s| has_text(view, s));
        let selection = sel.map(|s| selected_text(&doc, &s).trim().to_string());
        // Where the click was: the word under the pointer, else the text under it, else (the
        // menu opened from the keyboard with the pointer off the text) the selection's start.
        let at = req
            .word
            .as_ref()
            .map(|w| (w.unit, w.range.start))
            .or_else(|| req.pointer_at.map(|p| (p.unit, p.byte)))
            .or_else(|| sel.map(|s| s.range(&doc).0).map(|p| (p.unit, p.byte)));
        let word = req
            .word
            .clone()
            .or_else(|| {
                let (unit, byte) = at?;
                let (range, _) = doc.tagged_word_at(unit, byte)?;
                Some(WordHit { unit, range })
            })
            .and_then(|hit| {
                let shown = crate::word_info::shown_word(&self.session, &doc, &hit)?;
                let main = shown.main;
                let tagged_lemma =
                    doc.tagged_word_at(hit.unit, hit.range.start)
                        .is_some_and(|(_, parts)| {
                            parts
                                .iter()
                                .any(|p| p.strongs == main.strongs && p.lemma.is_some())
                        });
                Some(WordTarget {
                    hit,
                    strongs: main.strongs,
                    lemma: main.lemma,
                    tagged_lemma,
                    morphology: main.morphology.or(main.morph_code),
                })
            });
        let verse = verse_target(&doc, at, req.reference);
        let resource = Some((doc.manifest.resource_id.clone(), doc.manifest.resource_type));
        MenuTargets::new(
            selection,
            selected_verses(&doc, pane),
            verse,
            word,
            resource,
        )
    }

    /// Open Tab `tab`'s menu for what its pane asked: with nothing to act on, it stays shut
    /// (and never shows the last menu).
    pub(crate) fn open_context_menu(
        &mut self,
        tab: TabId,
        pane: &Entity<ReadingPane>,
        req: MenuRequest,
        cx: &mut Context<Self>,
    ) {
        let targets = self.reading_targets(pane.read(cx), &req);
        self.context_menu = (!targets.targets().is_empty()).then(|| ContextMenu {
            tab,
            targets,
            at: req.at,
            focus: cx.focus_handle(),
            focused: false,
        });
        cx.notify();
    }

    /// The open menu, if any.
    pub fn context_menu(&self) -> Option<&ContextMenu> {
        self.context_menu.as_ref()
    }

    /// Run a menu action, as the egui Shell's `menu_action`.
    pub fn menu_action(&mut self, action: MenuAction, new_tab: bool, cx: &mut Context<Self>) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        cx.notify();
        let tab = menu.tab;
        let m = &menu.targets;
        self.session.workspace.user_focused(tab);
        let selection = m.selection.clone().unwrap_or_default();
        let passage = m
            .verse
            .as_ref()
            .and_then(|v| v.standard)
            .map(|v| format_passage(Passage::verse(v), &Standard, RefStyle::Long));
        let strongs = m.word.as_ref().map(|w| w.strongs.to_string());
        let bible = m
            .resource
            .as_ref()
            .filter(|(_, ty)| *ty == ResourceType::Bible)
            .map(|(id, _)| id.clone());
        let open_at = |id: &'static str| (id, json!({ "passage": passage, "new_tab": new_tab }));
        let run: Option<(&'static str, serde_json::Value)> = match action {
            MenuAction::Copy => {
                if let Some(text) = self.pane(tab).and_then(|p| p.read(cx).copy_text()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                None
            }
            MenuAction::CopyVerses => {
                self.copy_verses_with_formatting(tab, m, cx);
                None
            }
            MenuAction::SearchThis => {
                let words: Vec<&str> = selection.split_whitespace().collect();
                match (&m.resource, words.is_empty()) {
                    (Some((id, ty)), false) => {
                        let query = format!("\"{}\"", words.join(" ").replace('"', ""));
                        Some((
                            "search.run",
                            if *ty == ResourceType::Bible {
                                json!({ "text": query, "mode": "bible", "bibles": id, "split": new_tab })
                            } else {
                                json!({ "text": query, "mode": "library", "resources": id, "split": new_tab })
                            },
                        ))
                    }
                    _ => None,
                }
            }
            MenuAction::LookUp => Some((
                "dictionary.lookup",
                json!({ "text": selection, "new_tab": new_tab }),
            )),
            MenuAction::ChooseWord => Some((
                "word_study.choose",
                json!({ "words": selection, "new_tab": new_tab }),
            )),
            MenuAction::PassageGuide => passage.is_some().then(|| open_at("guide.passage_guide")),
            MenuAction::ExegeticalGuide => {
                passage.is_some().then(|| open_at("guide.exegetical_guide"))
            }
            MenuAction::CrossReferences => {
                passage.is_some().then(|| open_at("tool.cross_references"))
            }
            MenuAction::TextComparison => {
                passage.is_some().then(|| open_at("tool.text_comparison"))
            }
            MenuAction::CopyVerse => {
                let text = self.pane(tab).and_then(|p| {
                    let doc = Arc::clone(&p.read(cx).view().doc);
                    let native = m.verse.as_ref()?.native?;
                    Some(copy_text(&doc, &verse_selection(&doc, native)?))
                });
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                None
            }
            MenuAction::WordStudy => strongs.map(|n| {
                (
                    "word_study.open",
                    json!({ "strongs": n, "new_tab": new_tab }),
                )
            }),
            MenuAction::OpenLexicon => {
                strongs.map(|n| ("lexicon.open", json!({ "strongs": n, "new_tab": new_tab })))
            }
            MenuAction::PinWordInfo => {
                let shown = m.word.as_ref().and_then(|w| {
                    let doc = Arc::clone(&self.pane(tab)?.read(cx).view().doc);
                    crate::word_info::shown_word(&self.session, &doc, &w.hit)
                });
                if let Some(shown) = shown {
                    self.session.word_info.pin(shown);
                    self.run("tool.word_info", &json!({}));
                }
                None
            }
            MenuAction::SearchLemma | MenuAction::SearchStrongs => {
                let query = match (action, m.word.as_ref()) {
                    (MenuAction::SearchLemma, Some(w)) if w.tagged_lemma => {
                        w.lemma.as_ref().map(|l| format!("lemma:{l}"))
                    }
                    (_, Some(w)) => Some(format!("strong:{}", w.strongs)),
                    _ => None,
                };
                query.map(|q| {
                    let mut params = json!({ "text": q, "mode": "bible", "split": new_tab });
                    if let Some(b) = &bible {
                        params["bibles"] = json!(b);
                    }
                    ("search.run", params)
                })
            }
            MenuAction::CopyMorphology => {
                if let Some(text) = m.word.as_ref().and_then(|w| w.morphology.clone()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                None
            }
        };
        if let Some((id, params)) = run {
            self.run_in(cx, id, &params);
        }
    }

    /// Copy verses with formatting (#154): the Verses the selection reaches, from this Bible,
    /// laid out by the Copy verses options in use, as plain text.
    fn copy_verses_with_formatting(&mut self, tab: TabId, m: &MenuTargets, cx: &mut App) {
        let Some(list) = m.selected_verses.as_ref() else {
            return;
        };
        let Some(doc) = self.pane(tab).map(|p| Arc::clone(&p.read(cx).view().doc)) else {
            return;
        };
        let bibles: [&dyn CopiedBible; 1] = [doc.as_ref()];
        let copied = obs_engine::copy_verses(list, &bibles, self.session.copy_verses.options);
        if copied.blocks.is_empty() {
            self.toast = Some("None of these Verses is in this Bible".into());
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(copied.plain_text()));
    }

    /// The open menu, drawn at where it was opened, over a backdrop a click on closes it.
    pub(crate) fn context_menu_overlay(
        &mut self,
        look: &Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.as_mut()?;
        if !menu.focused {
            menu.focused = true;
            window.focus(&menu.focus, cx);
        }
        let m = menu.targets.clone();
        let ch = look.chrome;
        let mut left = div()
            .flex()
            .flex_col()
            .gap(px(look.pt(space::XXS)))
            .min_w(px(look.pt(tokens::size::LABEL_COLUMN_WIDE)));
        for (i, target) in m.targets().into_iter().enumerate() {
            let value = m.value(target);
            let chosen = m.chosen == target;
            left = left.child(
                div()
                    .text_size(px(look.pt(tokens::text::SMALL)))
                    .text_color(tokens::rgba(ch.muted))
                    .child(target.kind()),
            );
            left = left.child(
                components::menu_item(
                    SharedString::from(format!("menu-target-{i}")),
                    value.clone(),
                    None,
                    look,
                )
                .accessibility_label(format!("{}: {value}", target.kind()))
                .selected(chosen)
                .when(chosen, |b| b.bg(tokens::rgba(ch.selection)))
                .on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
                    if *hovered && let Some(menu) = s.context_menu.as_mut() {
                        menu.targets.chosen = target;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |s, _, _, cx| {
                    if let Some(menu) = s.context_menu.as_mut() {
                        menu.targets.chosen = target;
                        cx.notify();
                    }
                })),
            );
        }
        let mut right = div().flex().flex_col();
        for (i, (action, label, enabled)) in m.actions().into_iter().enumerate() {
            right = right.child(
                components::menu_item(
                    SharedString::from(format!("menu-action-{i}")),
                    label,
                    None,
                    look,
                )
                .disabled(!enabled)
                .when(!enabled, |b| b.text_color(tokens::rgba(ch.muted)))
                .on_click(cx.listener(
                    move |s, ev: &gpui_kit::ClickEvent, _, cx| {
                        let m = ev.modifiers();
                        s.menu_action(action, m.control || m.platform, cx);
                    },
                )),
            );
        }
        let at = menu.at;
        let focus = menu.focus.clone();
        let close = cx.listener(|s, _: &MouseDownEvent, _, cx| {
            s.context_menu = None;
            cx.notify();
        });
        let close_right = cx.listener(|s, _: &MouseDownEvent, _, cx| {
            s.context_menu = None;
            cx.notify();
        });
        Some(
            div()
                .id("context-menu-backdrop")
                .absolute()
                .size_full()
                .occlude()
                .on_mouse_down(MouseButton::Left, close)
                .on_mouse_down(MouseButton::Right, close_right)
                .child(
                    components::menu("Context menu", look)
                        .track_focus(&focus)
                        .absolute()
                        .left(at.x)
                        .top(at.y)
                        .occlude()
                        // A click inside the menu is not a click on the backdrop.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(cx.listener(|s, ev: &KeyDownEvent, _, cx| {
                            if ev.keystroke.key == "escape" {
                                s.context_menu = None;
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }))
                        .flex()
                        .flex_row()
                        .gap(px(look.pt(space::M)))
                        .child(left)
                        .child(div().w(px(1.0)).self_stretch().bg(tokens::rgba(ch.rule)))
                        .child(right)
                        .test_support(),
                )
                .into_any_element(),
        )
    }
}

/// The Verse a reading pane's menu offers (the egui Shell's `verse_target`): the first target
/// of a Scripture reference under the pointer, else the Bible's or Commentary's Verse at the
/// click.
fn verse_target(
    doc: &ReadingDoc,
    at: Option<(usize, usize)>,
    reference: Option<(usize, usize)>,
) -> Option<VerseTarget> {
    if let Some((unit, span)) = reference {
        let (passage, label) = crate::references::first_target(doc, unit, span)?;
        return Some(VerseTarget {
            label,
            standard: Some(passage.start()),
            native: None,
        });
    }
    let (unit, byte) = at?;
    let verse = doc.verse_containing(unit, byte)?;
    if doc.manifest.resource_type == ResourceType::Bible {
        let standard = match to_standard(doc.verse_map.as_deref(), verse) {
            ToStandard::Verse(v) => Some(v),
            ToStandard::Unmapped => None,
        };
        return Some(VerseTarget {
            label: short_label(doc, verse),
            standard,
            native: Some(verse),
        });
    }
    // A Commentary's sections are keyed in the Standard versification.
    Some(VerseTarget {
        label: format_passage(Passage::verse(verse), &Standard, RefStyle::Short),
        standard: Some(verse),
        native: None,
    })
}
