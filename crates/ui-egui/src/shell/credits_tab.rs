//! Help → Credits (#67): a keyless, single-instance Tab listing every Starter Resource, the
//! Resources downloaded from the Open catalogue (#134), the data built into the app, the
//! bundled fonts with their licence texts, third-party crates with theirs, and the test data
//! in the source repository. The content comes from [`obs_engine::credits::credits`] and
//! `Session::open_resource_credits`.

use egui::{Align, CollapsingHeader, Layout, RichText, Ui, vec2};
use obs_engine::credits::{
    Credit, OPEN_RESOURCES_INTRO, OPEN_RESOURCES_TITLE, ThirdParty, changes_record_url, credits,
    published_changes_url,
};
use obs_engine::{Library, TabId};

use super::State;
use super::library_tab::section;
use crate::theme;
use crate::tokens::{self, size, space, stroke, text};

impl State {
    pub(super) fn credits_body(&mut self, ui: &mut Ui, tab: TabId) {
        let credits = credits();
        let downloaded = self.session.open_resource_credits();
        let library = &self.session.library;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .id_salt(("credits", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(crate::components::title_text("Credits"));
                    ui.label(
                        RichText::new(format!(
                            "Open Bible Software {} is built on open texts, open data, open \
                                 fonts and open-source software. Thank you to everyone credited \
                                 here.",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .color(theme::c().muted),
                    );
                    for p in &credits.problems {
                        ui.colored_label(theme::c().warn, p);
                    }
                    for (i, s) in credits.sections.iter().enumerate() {
                        ui.add_space(space::XL);
                        section(ui, s.title);
                        ui.label(RichText::new(s.intro).color(theme::c().muted));
                        for c in &s.credits {
                            ui.add_space(space::L);
                            credit_row(ui, c, library);
                        }
                        // Downloaded open Resources (#134) follow the Starter library.
                        if i == 0 && !downloaded.is_empty() {
                            ui.add_space(space::XL);
                            section(ui, OPEN_RESOURCES_TITLE);
                            ui.label(RichText::new(OPEN_RESOURCES_INTRO).color(theme::c().muted));
                            for c in &downloaded {
                                ui.add_space(space::L);
                                credit_row(ui, c, library);
                            }
                        }
                    }
                    ui.add_space(space::XL);
                    third_party(ui, &credits.third_party);
                });
        });
    }
}

/// One credited work: name, licence, source, credit, edition, use, changes and texts.
fn credit_row(ui: &mut Ui, c: &Credit, library: &Library) {
    ui.label(RichText::new(&c.name).strong().color(theme::c().ink));
    // The deed's link on a line of its own: a link wrapped across lines is not one
    // accessible node.
    field(ui, "Licence", |ui| {
        ui.label(&c.license);
        if let Some(url) = &c.license_url {
            ui.hyperlink_to("licence deed", url).on_hover_text(url);
        }
    });
    field(ui, "Source", |ui| linked_text(ui, &c.source));
    field(ui, "Attribution", |ui| linked_text(ui, &c.credit));
    if let Some(e) = &c.edition {
        field(ui, "Edition", |ui| {
            ui.label(e);
        });
    }
    if let Some(n) = &c.note {
        field(ui, "Used for", |ui| {
            ui.label(n);
        });
    }
    if let Some(ch) = &c.changes {
        field(ui, "Changes we made", |ui| {
            ui.label(ch);
        });
    }
    if let Some(id) = &c.changes_record {
        let url = library
            .get(id)
            .and_then(changes_record_url)
            .unwrap_or_else(|| published_changes_url(id));
        field(ui, "Change record", |ui| {
            ui.hyperlink_to(obs_model::changes_record_file(id), &url)
                .on_hover_text(&url);
        });
    }
    for (title, text) in &c.texts {
        CollapsingHeader::new(title.as_str())
            .id_salt(("credit-text", &c.name, title.as_str()))
            .show(ui, |ui| {
                ui.label(RichText::new(*text).monospace().size(text::CAPTION));
            });
    }
}

/// Width of the field-name column.
const FIELD_WIDTH: f32 = size::LABEL_COLUMN_WIDE;

/// A muted field name, then its value wrapped in the rest of the width.
fn field(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(FIELD_WIDTH, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(FIELD_WIDTH);
            ui.label(RichText::new(label).color(theme::c().muted));
        });
        let rest = ui.available_width().max(size::TEXT_MIN);
        ui.vertical(|ui| {
            ui.set_max_width(rest);
            value(ui);
        });
    });
}

/// `text` with every `http(s)://` URL in it made a link (e.g. "STEP Bible,
/// https://www.STEPBible.org"). One wrapped label, so long unbroken runs (commit hashes) wrap
/// inside the column; a click is mapped to the character under the pointer.
fn linked_text(ui: &mut Ui, text: &str) {
    let parts = split_urls(text);
    if !parts.iter().any(|(_, is_url)| *is_url) {
        ui.label(text);
        return;
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let ink = ui.visuals().text_color();
    let link = ui.visuals().hyperlink_color;
    let mut job = egui::text::LayoutJob::default();
    // Each URL as a character range of the label, and the URL.
    let mut urls: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut chars = 0;
    for (part, is_url) in parts {
        let format = if is_url {
            egui::TextFormat {
                font_id: font.clone(),
                color: link,
                underline: egui::Stroke::new(stroke::HAIR, link),
                ..Default::default()
            }
        } else {
            egui::TextFormat::simple(font.clone(), ink)
        };
        job.append(part, 0.0, format);
        let n = part.chars().count();
        if is_url {
            urls.push((chars..chars + n, part));
        }
        chars += n;
    }
    let (pos, galley, response) = egui::Label::new(job)
        .selectable(false)
        .sense(egui::Sense::click())
        .layout_in_ui(ui);
    let url_at = |p: egui::Pos2| -> Option<&str> {
        let at = galley.cursor_from_pos(p - pos).index.0;
        urls.iter()
            .find(|(range, _)| range.contains(&at))
            .map(|(_, url)| *url)
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
    let hovered = response.hover_pos().and_then(url_at);
    // From the keyboard (Enter or Space on the focused text) the first URL opens.
    let clicked = response
        .clicked()
        .then(|| match response.interact_pointer_pos() {
            Some(p) => url_at(p),
            None => urls.first().map(|(_, url)| *url),
        })
        .flatten();
    ui.painter().galley(pos, galley.clone(), ink);
    if let Some(url) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        response.on_hover_text(url);
    }
    if let Some(url) = clicked {
        ui.ctx().open_url(egui::OpenUrl::same_tab(url));
    }
}

/// Split `text` into (segment, is a URL), in order. A URL runs to the next whitespace, less
/// trailing punctuation.
fn split_urls(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = ["https://", "http://"]
        .iter()
        .filter_map(|p| rest.find(p))
        .min()
    {
        let tail = &rest[at..];
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ';', ':', ')', '"', '\'']);
        if at > 0 {
            out.push((&rest[..at], false));
        }
        out.push((url, true));
        rest = &tail[url.len()..];
    }
    if !rest.is_empty() || out.is_empty() {
        out.push((rest, false));
    }
    out
}

/// Third-party crates and their licence texts, generated by `cargo xtask licenses`.
fn third_party(ui: &mut Ui, t: &ThirdParty) {
    section(ui, "Third-party software");
    ui.label(
        RichText::new(format!(
            "The app is built with {} open-source Rust crates. Each is listed with the licence \
             it declares, followed by every licence text that covers them.",
            t.crates.len()
        ))
        .color(theme::c().muted),
    );
    CollapsingHeader::new(format!("Crates ({})", t.crates.len()))
        .id_salt("credits-crates")
        .show(ui, |ui| {
            egui::Grid::new("credits-crate-grid")
                .num_columns(3)
                .striped(true)
                .spacing(tokens::TABLE_SPACING)
                .show(ui, |ui| {
                    for c in &t.crates {
                        match &c.repository {
                            Some(r) => {
                                ui.hyperlink_to(&c.name, r).on_hover_text(r);
                            }
                            None => {
                                ui.label(&c.name);
                            }
                        }
                        ui.label(RichText::new(&c.version).color(theme::c().muted));
                        ui.label(&c.license);
                        ui.end_row();
                    }
                });
        });
    CollapsingHeader::new(format!("Licence texts ({})", t.licenses.len()))
        .id_salt("credits-licence-texts")
        .show(ui, |ui| {
            for (i, l) in t.licenses.iter().enumerate() {
                let users = match l.used_by.as_slice() {
                    [one] => one.clone(),
                    [first, rest @ ..] => format!("{first} and {} more", rest.len()),
                    [] => String::new(),
                };
                CollapsingHeader::new(format!("{} · {users}", l.name))
                    .id_salt(("credits-licence", i))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("Covers: {}", l.used_by.join(", ")))
                                .color(theme::c().muted),
                        );
                        ui.label(RichText::new(&l.text).monospace().size(text::CAPTION));
                    });
            }
        });
    if !t.generator.is_empty() {
        ui.add_space(space::S);
        ui.label(
            crate::components::caption_text(format!("Generated by {}.", t.generator))
                .color(theme::c().muted),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::split_urls;

    #[test]
    fn urls_in_credit_text_become_their_own_segments() {
        assert_eq!(
            split_urls("STEP Bible, https://www.STEPBible.org"),
            [("STEP Bible, ", false), ("https://www.STEPBible.org", true)]
        );
        assert_eq!(
            split_urls("See https://a.example/x. Then http://b.example/(y)"),
            [
                ("See ", false),
                ("https://a.example/x", true),
                (". Then ", false),
                ("http://b.example/(y", true),
                (")", false),
            ]
        );
        assert_eq!(split_urls("Public domain"), [("Public domain", false)]);
        assert_eq!(split_urls(""), [("", false)]);
    }
}
