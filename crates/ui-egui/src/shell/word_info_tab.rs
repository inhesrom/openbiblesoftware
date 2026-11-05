//! The Word Info Tool (#50, #18 rules 15–16): the Tagged word the pointer rests on (~150 ms
//! dwell, sticky) or the one clicked (📌 pinned). Shows the word as written, transliteration,
//! contextual gloss and reference; the main Lemma card (Lemma, Strong's number, plain-English
//! Morphology, the first Lexicon's gloss); and prefixes and suffixes under Parts. Original-
//! script text is drawn by the inline-snippet mode.

use egui::{Mesh, Pos2, Rect, RichText, Shape, Ui, pos2, vec2};
use obs_engine::word_info::lexicon_excerpt;
use obs_engine::{LemmaCard, LexiconGloss, ShownWord, TabId};
use obs_layout::{ColorRole, TextSettings};
use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, RefStyle, StrongsNumber, format_passage};
use serde_json::json;

use super::{Dwell, HOVER_DWELL_SECS, State};
use crate::pane::WordHit;
use crate::theme;

impl State {
    /// A reading pane reported what the pointer is on: start or continue the dwell, show the
    /// word once it has rested long enough, and pin it when clicked.
    pub(super) fn word_hovered(
        &mut self,
        ui: &Ui,
        tab: TabId,
        hovered: Option<WordHit>,
        clicked: Option<WordHit>,
    ) {
        if let Some(w) = &clicked
            && self.open_strongs_link(ui, tab, w)
        {
            return;
        }
        if let Some(w) = clicked
            && let Some(word) = self.shown_word(tab, &w)
        {
            self.session.word_info.pin(word);
            self.dwell = Some(Dwell {
                tab,
                word: w,
                since: self.now,
                shown: true,
            });
            return;
        }
        let Some(w) = hovered else {
            // Off every word: the panel keeps what it last showed (sticky).
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
            self.dwell = Some(Dwell {
                tab,
                word: w,
                since: self.now,
                shown: false,
            });
        }
        let Some(d) = self.dwell.as_mut() else {
            return;
        };
        if d.shown {
            return;
        }
        let rested = self.now - d.since;
        if rested < HOVER_DWELL_SECS {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(
                    HOVER_DWELL_SECS - rested,
                ));
            return;
        }
        d.shown = true;
        let word = d.word.clone();
        if let Some(shown) = self.shown_word(tab, &word) {
            self.session.word_info.hover(shown);
        }
    }

    /// A click on a Strong's link in a Lexicon, Dictionary or Commentary (a Tagged word
    /// outside a Bible) opens its Word Study (#55); Ctrl/Cmd-click opens a new one. Returns
    /// whether it did.
    fn open_strongs_link(&mut self, ui: &Ui, tab: TabId, hit: &WordHit) -> bool {
        let Some(doc) = self
            .tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .map(|v| std::sync::Arc::clone(&v.doc))
        else {
            return false;
        };
        if doc.manifest.resource_type == ResourceType::Bible {
            return false;
        }
        let Some((_, parts)) = doc.tagged_word_at(hit.unit, hit.range.start) else {
            return false;
        };
        let Some(number) = parts.first().map(|p| p.strongs) else {
            return false;
        };
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        self.session.workspace.user_focused(tab);
        self.run(
            "word_study.open",
            &json!({ "strongs": number.to_string(), "new_tab": new_tab }),
        );
        true
    }

    /// What Word Info shows for a Tagged word of a reading Tab.
    fn shown_word(&mut self, tab: TabId, hit: &WordHit) -> Option<ShownWord> {
        let doc = std::sync::Arc::clone(&self.tabs.get(&tab)?.view.as_ref()?.doc);
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
        let bible = doc.manifest.resource_id.clone();
        let mut shown = ShownWord::new(written, rtl, reference, parts, |n| self.lexicon_gloss(n))?;
        shown.bible = Some(bible);
        Some(shown)
    }

    /// The gloss of the first Lexicon in Preferred order for a Strong's number: its entry's
    /// first paragraph after the headword. The Lexicon is loaded like any other Resource.
    fn lexicon_gloss(&mut self, number: StrongsNumber) -> Option<LexiconGloss> {
        let entry = self.session.library.preferred(ResourceType::Lexicon)?;
        let (id, abbreviation) = (
            entry.manifest.resource_id.clone(),
            entry.manifest.abbreviation.clone(),
        );
        let doc = self.doc(&id).ok()?;
        let key = doc.navigation.lookup(&Key::StrongsNumber(number))?;
        let (unit, start) = doc.entry_position(key)?;
        let unit = doc.units.get(unit)?;
        let end = key.range.map_or(unit.text.len(), |(_, e)| e as usize);
        let text = lexicon_excerpt(unit.text.get(start..end)?)?;
        Some(LexiconGloss {
            resource_id: id,
            abbreviation,
            lemma: unit.title.clone(),
            text,
        })
    }

    pub(super) fn word_info_body(&mut self, ui: &mut Ui, _tab: TabId) {
        let Some(word) = self.session.word_info.shown.clone() else {
            egui::Frame::new()
                .fill(theme::c().field)
                .inner_margin(egui::Margin::symmetric(18, 14))
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(
                        RichText::new(
                            "Rest the pointer on a word in a Bible with Strong's numbers to see \
                             its Lemma, Strong's number, Morphology and Gloss here. Click a word \
                             to pin it.",
                        )
                        .color(theme::c().muted),
                    );
                });
            return;
        };
        let mut run: Option<&'static str> = None;
        let settings = self.session.text;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        // The word as written, large, through the pipeline (Hebrew and Greek
                        // need its fonts and shaping).
                        let big = TextSettings {
                            size: settings.size * 1.9,
                            ..settings
                        };
                        self.snippet(ui, &word.written, word.rtl, width, &big);
                        let mut line = Vec::new();
                        if let Some(t) = &word.transliteration {
                            line.push(t.clone());
                        }
                        if let Some(g) = &word.gloss {
                            line.push(format!("“{g}”"));
                        }
                        if !line.is_empty() {
                            ui.label(RichText::new(line.join("  ·  ")).color(theme::c().ink));
                        }
                        ui.label(RichText::new(&word.reference).color(theme::c().muted));
                        ui.add_space(10.0);
                        self.lemma_card(ui, &word.main, None, &settings);
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            for id in ["word_info.word_study", "word_info.search"] {
                                let label = self.session.commands().get(id).map_or(id, |c| c.label);
                                let enabled = self.session.is_enabled(id);
                                let r = ui.add_enabled(enabled, egui::Button::new(label));
                                if enabled {
                                    if r.clicked() {
                                        run = Some(id);
                                    }
                                } else {
                                    r.on_disabled_hover_text(super::not_yet(id));
                                }
                            }
                            if ui
                                .button("Copy")
                                .on_hover_text("Copy the word, its reference and its cards")
                                .clicked()
                            {
                                run = Some("word_info.copy");
                            }
                        });
                        if !word.parts.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("PARTS")
                                    .size(10.5)
                                    .strong()
                                    .color(theme::c().muted),
                            );
                            for p in &word.parts {
                                ui.add_space(4.0);
                                self.lemma_card(ui, &p.card, Some(p.role.name()), &settings);
                            }
                        }
                    });
            });
        if let Some(id) = run
            && let Some(out) = self.run(id, &json!({}))
            && id == "word_info.copy"
            && let Some(text) = out["text"].as_str()
        {
            ui.ctx().copy_text(text.to_string());
            self.toast("Copied");
        }
    }

    /// One Lemma card: Lemma (pipeline), Strong's number, Morphology (raw code in a tooltip)
    /// and the Lexicon gloss.
    fn lemma_card(
        &mut self,
        ui: &mut Ui,
        card: &LemmaCard,
        role: Option<&str>,
        settings: &TextSettings,
    ) {
        egui::Frame::new()
            .fill(theme::c().surface)
            .stroke(egui::Stroke::new(1.0, theme::c().rule))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some(role) = role {
                        ui.label(RichText::new(role).small().color(theme::c().muted));
                    }
                    ui.label(
                        RichText::new(card.strongs.to_string())
                            .strong()
                            .color(theme::c().lapis),
                    );
                    if let Some(g) = &card.gloss {
                        ui.label(RichText::new(format!("“{g}”")).color(theme::c().muted));
                    }
                });
                if let Some(lemma) = &card.lemma {
                    let rtl = card.strongs.language() == obs_scripture::StrongsLanguage::Hebrew;
                    let big = TextSettings {
                        size: settings.size * 1.3,
                        ..*settings
                    };
                    let width = ui.available_width();
                    self.snippet(ui, lemma, rtl, width, &big);
                }
                match (&card.morphology, &card.morph_code) {
                    (Some(m), code) => {
                        let r = ui.label(RichText::new(m).color(theme::c().ink));
                        if let Some(c) = code {
                            r.on_hover_text(format!("Morphology code: {c}"));
                        }
                    }
                    (None, Some(c)) => {
                        ui.label(RichText::new("Morphology not read").color(theme::c().muted))
                            .on_hover_text(format!("Morphology code: {c}"));
                    }
                    (None, None) => {}
                }
                match &card.lexicon {
                    Some(l) => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("{}:", l.abbreviation))
                                    .small()
                                    .color(theme::c().muted),
                            );
                            ui.label(RichText::new(&l.text).color(theme::c().ink));
                        });
                    }
                    None => {
                        ui.label(
                            RichText::new("No Lexicon entry")
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                }
            });
    }

    /// Draw `text` through the inline-snippet mode, as tall as its layout (a placeholder bar
    /// until a worker has laid it out).
    pub(super) fn snippet(
        &mut self,
        ui: &mut Ui,
        text: &str,
        rtl: bool,
        width: f32,
        settings: &TextSettings,
    ) {
        let width = width.max(40.0);
        let laid = self
            .snippets
            .get(text, ColorRole::Ink, rtl, width, settings);
        let height = laid
            .as_ref()
            .map_or(settings.size * settings.line_spacing, |l| l.height);
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
        let Some(laid) = laid else {
            ui.ctx().request_repaint();
            ui.painter().rect_filled(
                Rect::from_min_size(
                    pos2(rect.left(), rect.top() + height * 0.3),
                    vec2(width.min(settings.size * 6.0), height * 0.35),
                ),
                2.0,
                theme::c().rule,
            );
            return;
        };
        let ppp = ui.ctx().pixels_per_point();
        let quads = self
            .atlas
            .atlas
            .quads(&laid, [rect.left(), rect.top()], ppp);
        let texture = self.atlas.sync(ui.ctx());
        let mut mesh = Mesh::with_texture(texture);
        for q in quads {
            mesh.add_rect_with_uv(
                Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                theme::c().ink,
            );
        }
        ui.painter().add(Shape::mesh(mesh));
    }
}
