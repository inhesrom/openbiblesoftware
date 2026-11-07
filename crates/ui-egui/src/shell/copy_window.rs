//! Tools → Copy verses… (#154): a Passage list from one or more Bibles, laid out by the
//! Copy verses options or a Copy preset, previewed, then copied as plain text, rich text
//! (HTML with its plain-text form) or Markdown, or printed / saved as PDF.

use std::ops::Range;
use std::sync::Arc;

use egui::{Mesh, Pos2, Rect, RichText, Sense, Shape, Ui, vec2};
use obs_engine::copy_verses::OPTION_CHOICES;
use obs_engine::{
    CopiedBible, CopiedText, CopyFormat, CopyOptions, ReferencePlacement, TabKind, VerseLayout,
    html_document,
};
use obs_layout::{ColorRole, LaidBlock};
use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, format_passage, parse_reference};
use serde_json::json;

use super::State;
use crate::doc::ReadingDoc;
use crate::theme;
use crate::tokens::{self, inset, scale, share, size, space};
use crate::workers::SnippetCache;

/// The Copy verses window's egui id.
pub(crate) const WINDOW: &str = "copy-verses";

/// What a copy was made from: made again when any of it changes, including a chosen Bible
/// being updated or removed while the window is open.
#[derive(Debug, Clone, PartialEq)]
struct Made {
    passages: String,
    bibles: Vec<String>,
    /// Each chosen Bible's revision, content hash and file, `None` when not in the library.
    editions: Vec<Option<(u32, String, std::path::PathBuf)>>,
    options: CopyOptions,
}

/// One line of the preview, drawn by the text pipeline (ADR 0004): Hebrew lines read right
/// to left in the Reading font for Hebrew.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreviewLine {
    pub(crate) text: String,
    /// Byte ranges drawn in the verse-number colour (rich text's verse numbers).
    pub(crate) numbers: Vec<Range<usize>>,
    /// The line's first strong character reads right to left.
    pub(crate) rtl: bool,
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
    /// The preview's lines, laid out by the layout workers.
    snippets: SnippetCache,
    /// What the preview drew last frame, with the layouts of the lines laid out by then.
    pub(crate) shown: Vec<(PreviewLine, Option<Arc<LaidBlock>>)>,
}

impl CopyUi {
    /// The Bible `id` is gone from the library: it is no longer chosen.
    pub(super) fn removed(&mut self, id: &str) {
        self.bibles.retain(|b| b != id);
        self.made = None;
    }

    /// The Bible `id` was updated: copy its new text.
    pub(super) fn replaced(&mut self, id: &str) {
        if self.bibles.iter().any(|b| b == id) {
            self.made = None;
        }
    }

    /// Preview lines still being laid out.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn has_pending(&self) -> bool {
        self.snippets.has_pending()
    }
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
        let library = &self.session.library;
        let editions = (self.copy_ui.bibles.iter())
            .map(|id| {
                library.get(id).map(|e| {
                    let m = &e.manifest;
                    (m.revision, m.content_hash.clone(), e.path.clone())
                })
            })
            .collect();
        let made = Made {
            passages: self.copy_ui.passages.clone(),
            bibles: self.copy_ui.bibles.clone(),
            editions,
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
        let escape = super::escape_closes_window(ctx);
        let mut acts: Vec<Act> = Vec::new();
        let max_height = ctx.content_rect().height() * share::LIST;
        let window = egui::Window::new("Copy verses")
            .id(egui::Id::unique(WINDOW))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(size::DIALOG.x)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("copy-verses-choices")
                    .num_columns(2)
                    .spacing(tokens::GRID_SPACING)
                    .show(ui, |ui| {
                        ui.label("Passages");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.copy_ui.passages)
                                .id_salt("copy-passages")
                                .hint_text("Jn 3:16-18; Rom 5:8")
                                .desired_width(size::PROSE),
                        )
                        .accessible_name("Passages");
                        ui.end_row();
                        ui.label("Bibles");
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(size::PROSE);
                            for (id, abbrev, title) in &bibles {
                                let mut on = self.copy_ui.bibles.contains(id);
                                if crate::components::toggle(ui, &mut on, abbrev)
                                    .on_hover_text(title)
                                    .changed()
                                {
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
                                })
                                .response
                                .accessible_name("Preset");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.copy_ui.preset_name)
                                    .id_salt("copy-preset-name")
                                    .hint_text("Preset name")
                                    .desired_width(size::FIELD_NARROW),
                            )
                            .accessible_name("Preset name");
                            let name = self.copy_ui.preset_name.trim().to_string();
                            if ui
                                .add_enabled(
                                    !name.is_empty(),
                                    crate::components::button("Save preset"),
                                )
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
                    .inner_margin(inset::POPOVER)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .id_salt("copy-preview")
                            .max_height(max_height)
                            .show(ui, |ui| match &copied {
                                Ok(copied) => self.copy_preview(ui, copied),
                                Err(e) => {
                                    self.copy_ui.shown.clear();
                                    ui.label(RichText::new(e).color(theme::c().warn));
                                }
                            });
                    });
                ui.add_space(space::XS);
                ui.horizontal(|ui| {
                    let ok = copied.is_ok();
                    for (format, label) in [
                        (CopyFormat::PlainText, "Copy plain text"),
                        (CopyFormat::RichText, "Copy rich text"),
                        (CopyFormat::Markdown, "Copy Markdown"),
                    ] {
                        if ui
                            .add_enabled(ok, crate::components::button(label))
                            .clicked()
                        {
                            acts.push(Act::Copy(format));
                        }
                    }
                    if ui
                        .add_enabled(ok, crate::components::button("Print…"))
                        .on_hover_text("Opens in your browser, to print or save as PDF")
                        .clicked()
                    {
                        acts.push(Act::Print);
                    }
                });
            });
        if !open || escape(window.map(|r| r.response.layer_id)) {
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

impl State {
    /// The copy as it will paste, line by line through the text pipeline: plain text and
    /// Markdown as their text, rich text with its verse numbers marked.
    fn copy_preview(&mut self, ui: &mut Ui, copied: &CopiedText) {
        let lines = preview_lines(copied, self.copy_ui.preview);
        let settings = self.session.text;
        let width = ui.available_width().max(size::TEXT_MIN);
        let gap = settings.size * scale::PARAGRAPH_GAP;
        let mut shown = Vec::with_capacity(lines.len());
        for line in lines {
            if line.text.is_empty() {
                ui.add_space(gap);
                shown.push((line, None));
                continue;
            }
            let numbers: Vec<(Range<usize>, ColorRole)> = (line.numbers.iter())
                .map(|r| (r.clone(), ColorRole::VerseNumber))
                .collect();
            let laid = self.copy_ui.snippets.get_styled(
                &line.text,
                ColorRole::Ink,
                &numbers,
                line.rtl,
                width,
                &settings,
            );
            let height = laid
                .as_ref()
                .map_or(settings.size * settings.line_spacing, |l| l.height);
            let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let label = line.text.clone();
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
            match &laid {
                Some(laid) if ui.is_rect_visible(rect) => {
                    let quads = self.atlas.atlas.quads(
                        laid,
                        [rect.left(), rect.top()],
                        ui.ctx().pixels_per_point(),
                    );
                    let mut mesh = Mesh::with_texture(self.atlas.sync(ui.ctx()));
                    for q in quads {
                        mesh.add_rect_with_uv(
                            Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                            Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                            self.palette.color(q.role),
                        );
                    }
                    ui.painter().add(Shape::mesh(mesh));
                }
                Some(_) => {}
                None => ui.ctx().request_repaint(),
            }
            shown.push((line, laid));
        }
        self.copy_ui.shown = shown;
    }
}

/// The preview's lines for `format`; an empty line is a gap between paragraphs.
pub(crate) fn preview_lines(copied: &CopiedText, format: CopyFormat) -> Vec<PreviewLine> {
    let plain = |text: &str| PreviewLine {
        text: text.to_string(),
        numbers: Vec::new(),
        rtl: obs_layout::starts_rtl(text),
    };
    match format {
        CopyFormat::PlainText | CopyFormat::Markdown => {
            copied.render(format).split('\n').map(plain).collect()
        }
        CopyFormat::RichText => rich_text_lines(copied),
    }
}

/// Rich text as a word processor shows it, a line per paragraph: verse numbers marked
/// (joined to their verse by a no-break space), and the Attributions after the blocks.
fn rich_text_lines(copied: &CopiedText) -> Vec<PreviewLine> {
    let o = copied.options;
    let mut lines: Vec<PreviewLine> = Vec::new();
    let gap = |lines: &mut Vec<PreviewLine>| {
        if !lines.is_empty() {
            lines.push(PreviewLine {
                text: String::new(),
                numbers: Vec::new(),
                rtl: false,
            });
        }
    };
    let plain = |text: &str| PreviewLine {
        text: text.to_string(),
        numbers: Vec::new(),
        rtl: obs_layout::starts_rtl(text),
    };
    for block in &copied.blocks {
        if o.reference == ReferencePlacement::Before {
            gap(&mut lines);
            lines.push(plain(&block.reference));
        }
        let last_paragraph = block.paragraphs.len().saturating_sub(1);
        for (p, paragraph) in block.paragraphs.iter().enumerate() {
            if !(o.layout == VerseLayout::VersePerLine && p > 0) {
                gap(&mut lines);
            }
            let mut text = String::new();
            let mut numbers = Vec::new();
            if o.quotation_marks && p == 0 {
                text.push('“');
            }
            for (i, verse) in paragraph.iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                if o.verse_numbers && !verse.number.is_empty() {
                    numbers.push(text.len()..text.len() + verse.number.len());
                    text.push_str(&verse.number);
                    text.push('\u{a0}');
                }
                text.push_str(&verse.text);
            }
            if o.quotation_marks && p == last_paragraph {
                text.push('”');
            }
            let rtl = obs_layout::starts_rtl(&text);
            lines.push(PreviewLine { text, numbers, rtl });
        }
        if o.reference == ReferencePlacement::After {
            gap(&mut lines);
            lines.push(plain(&block.reference));
        }
    }
    for a in &copied.attributions {
        gap(&mut lines);
        lines.push(plain(a));
    }
    lines
}
