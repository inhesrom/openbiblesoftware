//! Tools → Copy verses… (#154): a Passage list from one or more Bibles, laid out by the
//! Copy verses options or a Copy preset, previewed, then copied as plain text, rich text
//! (HTML with its plain-text form) or Markdown, or printed / saved as PDF.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, RichText, Ui};
use obs_engine::copy_verses::OPTION_CHOICES;
use obs_engine::{CopiedBible, CopiedText, CopyFormat, CopyOptions, TabKind, html_document};
use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, format_passage, parse_reference};
use serde_json::json;

use super::State;
use crate::doc::ReadingDoc;
use crate::theme;

/// The Copy verses window's egui id.
pub(crate) const WINDOW: &str = "copy-verses";

/// What a copy was made from: made again when any of it changes.
#[derive(Debug, Clone, PartialEq)]
struct Made {
    passages: String,
    bibles: Vec<String>,
    options: CopyOptions,
}

/// The Copy verses window.
#[derive(Default)]
pub(crate) struct CopyUi {
    pub(super) open: bool,
    /// The Bible reference typed (Standard versification).
    pub(super) passages: String,
    /// The Bibles chosen, as resource ids, in the order picked.
    pub(super) bibles: Vec<String>,
    /// The preview's format.
    pub(super) preview: CopyFormat,
    /// The name typed for Save preset.
    preset_name: String,
    made: Option<(Made, Result<Arc<CopiedText>, String>)>,
}

/// One button's action.
enum Act {
    Option(&'static str, &'static str),
    Preset(&'static str, String),
    Copy(CopyFormat),
    Print,
}

impl State {
    /// Open Copy verses with `passages` and `bibles` when given; otherwise at the Current
    /// location of the most recently used Bible Tab, with that Bible chosen when none is.
    pub(super) fn open_copy_verses(&mut self, passages: Option<String>, bibles: Vec<String>) {
        self.copy_ui.open = true;
        self.copy_ui.made = None;
        if !bibles.is_empty() {
            self.copy_ui.bibles = bibles;
        }
        let given = passages.is_some();
        if let Some(passages) = passages {
            self.copy_ui.passages = passages;
        }
        let library = &self.session.library;
        let is_bible = |id: &str| {
            library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
        };
        let recent = self
            .session
            .workspace
            .most_recent(|t| matches!(&t.kind, TabKind::Resource(id) if is_bible(id)))
            .and_then(|t| self.session.workspace.tab(t));
        let mut bible = None;
        if let Some(t) = recent
            && let TabKind::Resource(id) = &t.kind
        {
            bible = Some(id.clone());
            if !given && let Some(native) = t.history.current() {
                let standard = match &t.verse_map {
                    Some(map) => map.to_standard(native).map(|p| p.start()),
                    None => Some(native),
                };
                if let Some(v) = standard {
                    self.copy_ui.passages =
                        format_passage(Passage::verse(v), &Standard, RefStyle::Short);
                }
            }
        }
        self.copy_ui.bibles.retain(|id| is_bible(id));
        if self.copy_ui.bibles.is_empty() {
            let first = bible.or_else(|| {
                library
                    .preferred(ResourceType::Bible)
                    .map(|e| e.manifest.resource_id.clone())
            });
            self.copy_ui.bibles.extend(first);
        }
    }

    /// The copy for the window's choices, made again only when they change.
    fn copied(&mut self) -> Result<Arc<CopiedText>, String> {
        let made = Made {
            passages: self.copy_ui.passages.clone(),
            bibles: self.copy_ui.bibles.clone(),
            options: self.session.copy_verses.options,
        };
        if let Some((from, result)) = &self.copy_ui.made
            && *from == made
        {
            return result.clone();
        }
        let result = self.make_copy(&made);
        self.copy_ui.made = Some((made, result.clone()));
        result
    }

    fn make_copy(&mut self, made: &Made) -> Result<Arc<CopiedText>, String> {
        if made.passages.trim().is_empty() {
            return Err("Type the Passages to copy, such as Jn 3:16-18; Rom 5:8".into());
        }
        let list = parse_reference(&made.passages, &Standard, None).map_err(|e| e.to_string())?;
        if made.bibles.is_empty() {
            return Err("Choose at least one Bible".into());
        }
        let mut docs: Vec<Arc<ReadingDoc>> = Vec::new();
        for id in &made.bibles {
            docs.push(self.doc(id)?);
        }
        let bibles: Vec<&dyn CopiedBible> = docs
            .iter()
            .map(|d| d.as_ref() as &dyn CopiedBible)
            .collect();
        let copied = obs_engine::copy_verses(&list, &bibles, made.options);
        if copied.blocks.is_empty() {
            return Err("None of these Passages is in the chosen Bibles".into());
        }
        Ok(Arc::new(copied))
    }

    /// The Copy verses window.
    pub(super) fn copy_window(&mut self, ctx: &egui::Context) {
        if !self.copy_ui.open {
            return;
        }
        let copied = self.copied();
        let options = self.session.copy_verses.options;
        let current = self
            .session
            .copy_verses
            .current_preset()
            .map(|p| p.name.clone());
        let presets: Vec<String> = (self.session.copy_verses.presets.iter())
            .map(|p| p.name.clone())
            .collect();
        let bibles: Vec<(String, String, String)> = self
            .session
            .library
            .of_type(ResourceType::Bible)
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    e.manifest.abbreviation.clone(),
                    e.manifest.title.clone(),
                )
            })
            .collect();
        let mut open = true;
        let mut acts: Vec<Act> = Vec::new();
        let max_height = ctx.content_rect().height() * 0.4;
        egui::Window::new("Copy verses")
            .id(egui::Id::unique(WINDOW))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("copy-verses-choices")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Passages");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.copy_ui.passages)
                                .id_salt("copy-passages")
                                .hint_text("Jn 3:16-18; Rom 5:8")
                                .desired_width(420.0),
                        );
                        ui.end_row();
                        ui.label("Bibles");
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(460.0);
                            for (id, abbrev, title) in &bibles {
                                let mut on = self.copy_ui.bibles.contains(id);
                                if ui.checkbox(&mut on, abbrev).on_hover_text(title).changed() {
                                    if on {
                                        self.copy_ui.bibles.push(id.clone());
                                    } else {
                                        self.copy_ui.bibles.retain(|b| b != id);
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Preset");
                        ui.horizontal(|ui| {
                            let shown = current.clone().unwrap_or_else(|| "Custom".into());
                            egui::ComboBox::from_id_salt("copy-preset")
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    if presets.is_empty() {
                                        ui.label(
                                            RichText::new("No presets saved yet")
                                                .color(theme::c().muted),
                                        );
                                    }
                                    for name in &presets {
                                        let on = current.as_deref() == Some(name.as_str());
                                        if ui.selectable_label(on, name).clicked() && !on {
                                            acts.push(Act::Preset(
                                                "copy.preset_apply",
                                                name.clone(),
                                            ));
                                        }
                                    }
                                });
                            ui.add(
                                egui::TextEdit::singleline(&mut self.copy_ui.preset_name)
                                    .id_salt("copy-preset-name")
                                    .hint_text("Preset name")
                                    .desired_width(130.0),
                            );
                            let name = self.copy_ui.preset_name.trim().to_string();
                            if ui
                                .add_enabled(!name.is_empty(), egui::Button::new("Save preset"))
                                .clicked()
                            {
                                acts.push(Act::Preset("copy.preset_save", name));
                            }
                            if let Some(name) = &current
                                && ui.button("Delete preset").clicked()
                            {
                                acts.push(Act::Preset("copy.preset_delete", name.clone()));
                            }
                        });
                        ui.end_row();
                        for (name, label, values) in OPTION_CHOICES {
                            ui.label(*label);
                            ui.horizontal(|ui| {
                                for (value, value_label) in *values {
                                    let on = options.value(name) == Some(*value);
                                    if ui.radio(on, *value_label).clicked() && !on {
                                        acts.push(Act::Option(name, value));
                                    }
                                }
                            });
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Preview").strong());
                    for format in CopyFormat::ALL {
                        ui.selectable_value(&mut self.copy_ui.preview, format, format.label());
                    }
                });
                egui::Frame::new()
                    .fill(theme::c().surface)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .id_salt("copy-preview")
                            .max_height(max_height)
                            .show(ui, |ui| match &copied {
                                Ok(copied) => preview(ui, copied, self.copy_ui.preview),
                                Err(e) => {
                                    ui.label(RichText::new(e).color(theme::c().warn));
                                }
                            });
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let ok = copied.is_ok();
                    for (format, label) in [
                        (CopyFormat::PlainText, "Copy plain text"),
                        (CopyFormat::RichText, "Copy rich text"),
                        (CopyFormat::Markdown, "Copy Markdown"),
                    ] {
                        if ui.add_enabled(ok, egui::Button::new(label)).clicked() {
                            acts.push(Act::Copy(format));
                        }
                    }
                    if ui
                        .add_enabled(ok, egui::Button::new("Print…"))
                        .on_hover_text("Opens in your browser, to print or save as PDF")
                        .clicked()
                    {
                        acts.push(Act::Print);
                    }
                });
            });
        if !open {
            self.copy_ui.open = false;
        }
        for act in acts {
            match act {
                Act::Option(name, value) => {
                    self.run("copy.option", &json!({ "name": name, "value": value }));
                }
                Act::Preset(command, name) => {
                    if self.run(command, &json!({ "name": name })).is_some()
                        && command == "copy.preset_save"
                    {
                        self.copy_ui.preset_name.clear();
                    }
                }
                Act::Copy(format) => {
                    if let Ok(copied) = &copied {
                        self.copy_to_clipboard(ctx, copied, format);
                    }
                }
                Act::Print => {
                    if let Ok(copied) = &copied {
                        let title = self.copy_ui.passages.trim().to_string();
                        let page = html_document(&title, &copied.html(), true);
                        self.open_print_page(ctx, &title, &page);
                    }
                }
            }
        }
    }

    fn copy_to_clipboard(&mut self, ctx: &egui::Context, copied: &CopiedText, format: CopyFormat) {
        let plain = copied.plain_text();
        match format {
            CopyFormat::RichText => match &self.rich_clipboard {
                Some(clipboard) => match clipboard.copy_html(&copied.html(), &plain) {
                    Ok(()) => self.toast("Copied as rich text"),
                    Err(e) => {
                        ctx.copy_text(plain);
                        self.toast(format!(
                            "Rich text could not be copied ({e}); copied as plain text"
                        ));
                    }
                },
                None => {
                    ctx.copy_text(plain);
                    self.toast("Rich text cannot be copied here; copied as plain text");
                }
            },
            CopyFormat::PlainText => {
                ctx.copy_text(plain);
                self.toast("Copied as plain text");
            }
            CopyFormat::Markdown => {
                ctx.copy_text(copied.markdown());
                self.toast("Copied as Markdown");
            }
        }
    }
}

/// The copy as it will paste: plain text and Markdown as text, rich text formatted.
fn preview(ui: &mut Ui, copied: &CopiedText, format: CopyFormat) {
    match format {
        CopyFormat::PlainText => {
            ui.add(egui::Label::new(
                RichText::new(copied.plain_text()).color(theme::c().ink),
            ));
        }
        CopyFormat::Markdown => {
            ui.add(egui::Label::new(
                RichText::new(copied.markdown())
                    .monospace()
                    .color(theme::c().ink),
            ));
        }
        CopyFormat::RichText => {
            ui.add(egui::Label::new(rich_text(ui, copied)));
        }
    }
}

/// Rich text as a word processor shows it: verse numbers raised and small, the Attribution
/// small.
fn rich_text(ui: &Ui, copied: &CopiedText) -> LayoutJob {
    let size = ui.style().text_styles[&egui::TextStyle::Body].size;
    let ink = theme::c().ink;
    let text = TextFormat::simple(FontId::proportional(size), ink);
    let small = TextFormat::simple(FontId::proportional(size * 0.8), theme::c().muted);
    let raised = TextFormat {
        font_id: FontId::proportional(size * 0.65),
        color: ink,
        valign: egui::Align::TOP,
        ..Default::default()
    };
    let o = copied.options;
    let mut job = LayoutJob::default();
    let gap = |job: &mut LayoutJob| {
        if !job.text.is_empty() {
            job.append("\n\n", 0.0, text.clone());
        }
    };
    for block in &copied.blocks {
        if o.reference == obs_engine::ReferencePlacement::Before {
            gap(&mut job);
            job.append(&block.reference, 0.0, text.clone());
        }
        let last_paragraph = block.paragraphs.len().saturating_sub(1);
        for (p, paragraph) in block.paragraphs.iter().enumerate() {
            let together = o.layout == obs_engine::VerseLayout::VersePerLine && p > 0;
            if together {
                job.append("\n", 0.0, text.clone());
            } else {
                gap(&mut job);
            }
            if o.quotation_marks && p == 0 {
                job.append("“", 0.0, text.clone());
            }
            for (i, verse) in paragraph.iter().enumerate() {
                if i > 0 {
                    job.append(" ", 0.0, text.clone());
                }
                if o.verse_numbers && !verse.number.is_empty() {
                    job.append(&verse.number, 0.0, raised.clone());
                    job.append("\u{a0}", 0.0, text.clone());
                }
                job.append(&verse.text, 0.0, text.clone());
            }
            if o.quotation_marks && p == last_paragraph {
                job.append("”", 0.0, text.clone());
            }
        }
        if o.reference == obs_engine::ReferencePlacement::After {
            gap(&mut job);
            job.append(&block.reference, 0.0, text.clone());
        }
    }
    for a in &copied.attributions {
        gap(&mut job);
        job.append(a, 0.0, small.clone());
    }
    job.wrap.max_width = ui.available_width();
    job
}
