//! View → Fonts (#138): a reading font per script (Latin, Greek, Hebrew) and the interface
//! font, each the bundled one or a font installed on this computer. Choices are stored by name
//! in the User database (`view.font`); here they are loaded, checked for coverage and handed
//! to layout, which falls back to the bundled faces for anything a chosen font lacks.

use std::collections::HashMap;
use std::sync::Arc;

use egui::RichText;
use obs_engine::{FontSlot, Session};
use obs_fonts::{
    Coverage, FaceId, FaceSet, ReadingFaces, ReadingScript, ShapeCache, face, load_user_face,
    script_coverage, unload_user_face,
};
use serde_json::json;

use super::State;
use crate::system_fonts::{InstalledFamily, NoSystemFonts, SystemFonts};
use crate::theme;

/// Family and style chosen for each reading script (Latin, Greek, Hebrew).
type ReadingWanted = [Option<(String, String)>; 3];

/// Jn 1:1 in English and Greek, Gen 1:1 in Hebrew: the preview of each reading font.
const PREVIEWS: [(ReadingScript, &str, bool); 3] = [
    (
        ReadingScript::Latin,
        "In the beginning was the Word, and the Word was with God, and the Word was God.",
        false,
    ),
    (
        ReadingScript::Greek,
        "Ἐν ἀρχῇ ἦν ὁ λόγος, καὶ ὁ λόγος ἦν πρὸς τὸν θεόν, καὶ θεὸς ἦν ὁ λόγος.",
        false,
    ),
    (
        ReadingScript::Hebrew,
        "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃",
        true,
    ),
];

/// The Fonts window and the fonts loaded for the current choices.
pub(crate) struct FontsUi {
    pub(super) source: Arc<dyn SystemFonts>,
    pub(super) open: bool,
    /// The choices the loaded faces were made for (`None`: not applied yet).
    applied_reading: Option<ReadingWanted>,
    applied_interface: Option<Option<(String, String)>>,
    /// User faces loaded for the reading fonts, unloaded when the choice changes.
    loaded: Vec<FaceId>,
    /// Installed families, read when the window first opens.
    families: Option<Arc<Vec<InstalledFamily>>>,
    filter: String,
    /// Slots whose chosen font is not installed here.
    missing: [bool; 4],
}

impl Default for FontsUi {
    fn default() -> Self {
        Self {
            source: Arc::new(NoSystemFonts),
            open: false,
            applied_reading: None,
            applied_interface: None,
            loaded: Vec::new(),
            families: None,
            filter: String::new(),
            missing: [false; 4],
        }
    }
}

impl FontsUi {
    /// Use another source of installed fonts; the choices are loaded again from it.
    pub(super) fn set_source(&mut self, source: Arc<dyn SystemFonts>) {
        self.source = source;
        self.applied_reading = None;
        self.applied_interface = None;
        self.families = None;
    }
}

fn wanted(s: &Session, slot: FontSlot) -> Option<(String, String)> {
    s.fonts
        .get(slot)
        .map(|c| (c.family.clone(), c.style.clone()))
}

/// The four faces of a chosen family, loaded (each file once, by key). `None` when the family
/// is not installed or its upright face cannot be read.
fn load_set(
    source: &dyn SystemFonts,
    family: &str,
    style: &str,
    by_key: &mut HashMap<String, FaceId>,
) -> Option<FaceSet> {
    let mut load = |bold: bool, italic: bool| -> Option<FaceId> {
        let file = source.load(family, style, bold, italic)?;
        if let Some(id) = by_key.get(&file.key) {
            return Some(*id);
        }
        let id = load_user_face(file.data, file.index).ok()?;
        by_key.insert(file.key, id);
        Some(id)
    };
    let regular = load(false, false)?;
    Some(FaceSet {
        regular,
        italic: load(false, true).unwrap_or(regular),
        bold: load(true, false).unwrap_or(regular),
        bold_italic: load(true, true).unwrap_or(regular),
    })
}

fn missing_notice(slot: FontSlot, family: &str, style: &str) -> String {
    let what = match slot.script() {
        Some(s) => format!("for {} text", s.label()),
        None => "for the interface".into(),
    };
    format!(
        "The font “{family} {style}” is not installed on this computer; {} is used {what}.",
        slot.bundled_family()
    )
}

impl State {
    /// Load the chosen fonts when the choice changed (and on the first frame): reading faces go
    /// into the Text settings, so every pane lays its text out again; the interface font goes
    /// into egui's fonts. A font not installed here falls back to the bundled one, with a
    /// notice the first time.
    pub(super) fn apply_fonts(&mut self, ctx: &egui::Context) {
        let reading = [FontSlot::Latin, FontSlot::Greek, FontSlot::Hebrew]
            .map(|slot| wanted(&self.session, slot));
        let interface = wanted(&self.session, FontSlot::Interface);
        let source = Arc::clone(&self.fonts_ui.source);
        let mut notices = Vec::new();
        if self.fonts_ui.applied_reading.as_ref() != Some(&reading) {
            let mut by_key = HashMap::new();
            let mut faces = ReadingFaces::default();
            for (i, slot) in [FontSlot::Latin, FontSlot::Greek, FontSlot::Hebrew]
                .into_iter()
                .enumerate()
            {
                let (Some((family, style)), Some(script)) = (&reading[i], slot.script()) else {
                    self.fonts_ui.missing[slot as usize] = false;
                    continue;
                };
                let set = load_set(&*source, family, style, &mut by_key);
                self.fonts_ui.missing[slot as usize] = set.is_none();
                match set {
                    Some(set) => {
                        faces.set(script, set);
                        self.session.font_found(slot);
                    }
                    None if self.session.font_missing(slot) => {
                        notices.push(missing_notice(slot, family, style));
                    }
                    None => {}
                }
            }
            for id in std::mem::take(&mut self.fonts_ui.loaded) {
                unload_user_face(id);
            }
            self.fonts_ui.loaded = by_key.into_values().collect();
            if self.session.text.fonts != faces {
                self.session.text.fonts = faces;
                // Shaped segments of the old faces are never asked for again.
                ShapeCache::shared().clear();
            }
            self.fonts_ui.applied_reading = Some(reading);
        }
        if self.fonts_ui.applied_interface.as_ref() != Some(&interface) {
            let first = self.fonts_ui.applied_interface.is_none();
            let slot = FontSlot::Interface;
            let data = interface.as_ref().and_then(|(family, style)| {
                let file = source.load(family, style, false, false);
                self.fonts_ui.missing[slot as usize] = file.is_none();
                match &file {
                    Some(_) => self.session.font_found(slot),
                    None if self.session.font_missing(slot) => {
                        notices.push(missing_notice(slot, family, style));
                    }
                    None => {}
                }
                let file = file?;
                let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = file.data;
                Some(egui::FontData::from_blob(blob, file.index))
            });
            if interface.is_none() {
                self.fonts_ui.missing[slot as usize] = false;
            }
            // The bundled interface fonts are installed at start-up already.
            if !(first && data.is_none()) {
                theme::install_fonts(ctx, data);
            }
            self.fonts_ui.applied_interface = Some(interface);
        }
        if !notices.is_empty() {
            self.toast(notices.join("\n"));
        }
    }

    /// What the window says about a reading font's coverage of its script, if anything.
    fn coverage_note(&self, script: ReadingScript) -> Option<String> {
        let id = self.session.text.fonts.get(script).regular;
        if id.is_bundled() {
            return None;
        }
        let f = face(id).ok()?;
        let (name, bundled) = (script.label(), script.bundled_family());
        match script_coverage(&f, script) {
            Coverage::Full => None,
            Coverage::None => Some(format!(
                "This font has no {name} letters; {bundled} will be used."
            )),
            Coverage::Partial { missing } => Some(format!(
                "This font lacks {missing} {name} character{}; {bundled} draws {}.",
                if missing == 1 { "" } else { "s" },
                if missing == 1 { "it" } else { "those" },
            )),
        }
    }

    /// The Fonts window (View → Fonts…).
    pub(super) fn fonts_window(&mut self, ctx: &egui::Context) {
        if !self.fonts_ui.open {
            return;
        }
        let source = Arc::clone(&self.fonts_ui.source);
        let families = Arc::clone(
            self.fonts_ui
                .families
                .get_or_insert_with(|| Arc::new(source.families())),
        );
        let mut open = true;
        let mut picks: Vec<(FontSlot, Option<(String, String)>)> = Vec::new();
        let max_height = ctx.content_rect().height() * 0.75;
        egui::Window::new("Fonts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(580.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Choose a font for each script and for the interface. Letters a font \
                         lacks are drawn in the bundled font.",
                    )
                    .color(theme::c().muted),
                );
                ui.horizontal(|ui| {
                    ui.label("Filter installed fonts");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.fonts_ui.filter)
                            .id_salt("fonts-filter")
                            .desired_width(200.0),
                    );
                });
                if families.is_empty() {
                    ui.label(
                        RichText::new(
                            "No installed fonts were found; only the bundled fonts can be used.",
                        )
                        .color(theme::c().muted),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .min_scrolled_height(max_height.min(640.0))
                    .show(ui, |ui| {
                        for slot in FontSlot::ALL {
                            ui.add_space(10.0);
                            self.font_slot_ui(ui, slot, &families, &mut picks);
                        }
                    });
            });
        if !open {
            self.fonts_ui.open = false;
        }
        for (slot, pick) in picks {
            let (family, style) = pick.unwrap_or_default();
            self.run(
                "view.font",
                &json!({ "slot": slot.id(), "family": family, "style": style }),
            );
        }
    }

    fn font_slot_ui(
        &mut self,
        ui: &mut egui::Ui,
        slot: FontSlot,
        families: &[InstalledFamily],
        picks: &mut Vec<(FontSlot, Option<(String, String)>)>,
    ) {
        let bundled = match slot.script() {
            Some(_) => format!("{} (bundled)", slot.bundled_family()),
            None => "Bundled interface font".to_string(),
        };
        let current = wanted(&self.session, slot);
        ui.label(RichText::new(slot.label()).strong().color(theme::c().ink));
        ui.horizontal(|ui| {
            let family_text = current.as_ref().map_or(bundled.clone(), |c| c.0.clone());
            egui::ComboBox::from_id_salt(("font-family", slot.id()))
                .selected_text(family_text)
                .width(260.0)
                .height(360.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), &bundled).clicked() {
                        picks.push((slot, None));
                    }
                    ui.separator();
                    let filter = self.fonts_ui.filter.trim().to_lowercase();
                    for f in families
                        .iter()
                        .filter(|f| filter.is_empty() || f.name.to_lowercase().contains(&filter))
                    {
                        let on = current.as_ref().is_some_and(|c| c.0 == f.name);
                        if ui.selectable_label(on, &f.name).clicked() {
                            let style = f
                                .styles
                                .iter()
                                .find(|s| s.as_str() == "Regular")
                                .or(f.styles.first())
                                .cloned()
                                .unwrap_or_else(|| "Regular".into());
                            picks.push((slot, Some((f.name.clone(), style))));
                        }
                    }
                });
            if let Some((family, style)) = &current {
                let styles = families
                    .iter()
                    .find(|f| &f.name == family)
                    .map(|f| f.styles.clone())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt(("font-style", slot.id()))
                    .selected_text(style.as_str())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for st in styles {
                            if ui.selectable_label(&st == style, &st).clicked() {
                                picks.push((slot, Some((family.clone(), st))));
                            }
                        }
                    });
            }
        });
        if self.fonts_ui.missing[slot as usize] {
            ui.label(
                RichText::new(format!(
                    "Not installed on this computer; {} is used.",
                    slot.bundled_family()
                ))
                .color(theme::c().warn),
            );
        }
        match slot.script() {
            Some(script) => {
                if let Some(note) = self.coverage_note(script) {
                    ui.label(RichText::new(note).color(theme::c().warn));
                }
                if let Some((_, sample, rtl)) = PREVIEWS.iter().find(|p| p.0 == script) {
                    let settings = self.session.text;
                    let width = ui.available_width().min(540.0);
                    egui::Frame::new()
                        .fill(theme::c().field)
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            self.snippet(ui, sample, *rtl, width - 20.0, &settings);
                        });
                }
            }
            None => {
                ui.label(
                    RichText::new("Menus, buttons and labels: Library · Guides · Tools ★")
                        .color(theme::c().ink),
                );
            }
        }
    }
}
