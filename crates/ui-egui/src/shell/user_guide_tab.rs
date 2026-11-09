//! Help → User guide (#218): the offline guide's pages in one Tab. The pages are listed in a
//! column (a row of links when the Tab is narrow); the page shown is drawn by the Notes
//! renderer ([`State::body_line`], Note formatting) with its screenshots between its
//! paragraphs. Typing in the header's box searches every page: each page that holds every word
//! is listed with the lines that hold one, and clicking it shows the page. Links to other pages
//! show them; Bible references preview and open as in a Note.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{RichText, Ui, vec2};
use obs_engine::{
    GuideBlock, GuideMatch, GuidePage, TabId, USER_GUIDE, guide_blocks, search_user_guide,
};
use serde_json::{Value, json};

use super::State;
use crate::theme;
use crate::tokens::{size, space};

/// The screenshots the guide shows, compiled in: their file names in `docs/previews/` (the
/// screenshot gallery) and their PNG bytes.
pub(crate) const GUIDE_IMAGES: &[(&str, &[u8])] = &[
    (
        "user-guide-window.png",
        include_bytes!("../../../../docs/previews/user-guide-window.png"),
    ),
    (
        "user-guide-reading.png",
        include_bytes!("../../../../docs/previews/user-guide-reading.png"),
    ),
    (
        "user-guide-link-sets.png",
        include_bytes!("../../../../docs/previews/user-guide-link-sets.png"),
    ),
    (
        "user-guide-passage-guide.png",
        include_bytes!("../../../../docs/previews/user-guide-passage-guide.png"),
    ),
    (
        "user-guide-word-study.png",
        include_bytes!("../../../../docs/previews/user-guide-word-study.png"),
    ),
    (
        "user-guide-search.png",
        include_bytes!("../../../../docs/previews/user-guide-search.png"),
    ),
    (
        "user-guide-notes.png",
        include_bytes!("../../../../docs/previews/user-guide-notes.png"),
    ),
    (
        "user-guide-get-resources.png",
        include_bytes!("../../../../docs/previews/user-guide-get-resources.png"),
    ),
    (
        "user-guide-layouts.png",
        include_bytes!("../../../../docs/previews/user-guide-layouts.png"),
    ),
    (
        "user-guide-keyboard.png",
        include_bytes!("../../../../docs/previews/user-guide-keyboard.png"),
    ),
];

/// Narrower than this, the pages are a row of links above the page instead of a column.
const COLUMN_TAB: f32 = size::SIDEBAR_TAB;

/// A command the Tab asked for.
type Run = Vec<(&'static str, Value)>;

/// The User guide Tab's state across frames.
#[derive(Default)]
pub(crate) struct UserGuideUi {
    /// Each page's blocks, parsed the first time it is shown.
    blocks: HashMap<&'static str, Arc<Vec<GuideBlock>>>,
    /// The last search and what it found, kept while the query is unchanged.
    search: Option<(String, Arc<Vec<GuideMatch>>)>,
    /// Decoded screenshots, by file name (`None`: not compiled in, or not a PNG).
    textures: HashMap<String, Option<egui::TextureHandle>>,
    /// What the page or the search results showed last frame: each line's text and each
    /// screenshot's description, in order (for tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) drawn: Vec<String>,
}

impl UserGuideUi {
    fn blocks(&mut self, page: &'static GuidePage) -> Arc<Vec<GuideBlock>> {
        Arc::clone(
            (self.blocks)
                .entry(page.id)
                .or_insert_with(|| Arc::new(guide_blocks(page))),
        )
    }

    /// What `query` finds, searched again only when the query changes.
    fn search(&mut self, query: &str) -> Arc<Vec<GuideMatch>> {
        match &self.search {
            Some((q, found)) if q == query => Arc::clone(found),
            _ => {
                let found = Arc::new(search_user_guide(query));
                self.search = Some((query.to_string(), Arc::clone(&found)));
                found
            }
        }
    }

    /// Whether the screenshot `file` has been decoded and uploaded (for tests).
    #[cfg(test)]
    pub(crate) fn has_texture(&self, file: &str) -> bool {
        self.textures.get(file).is_some_and(Option::is_some)
    }

    /// The screenshot `file`, decoded and uploaded the first time it is shown.
    fn texture(&mut self, ctx: &egui::Context, file: &str) -> Option<egui::TextureHandle> {
        (self.textures.entry(file.to_string()))
            .or_insert_with(|| {
                let bytes = GUIDE_IMAGES.iter().find(|(f, _)| *f == file)?.1;
                let image = decode_png(bytes)?;
                Some(ctx.load_texture(
                    format!("user-guide/{file}"),
                    image,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .clone()
    }
}

/// A PNG as an egui image.
pub(crate) fn decode_png(bytes: &[u8]) -> Option<egui::ColorImage> {
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(bytes).ok()?;
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    Some(egui::ColorImage::from_rgba_premultiplied(
        size,
        pixmap.data(),
    ))
}

impl State {
    pub(super) fn user_guide_body(&mut self, ui: &mut Ui, tab: TabId) {
        let query = (self.tabs.get(&tab))
            .map(|t| t.filter.trim().to_string())
            .unwrap_or_default();
        let page = self.session.user_guide().page();
        self.user_guide_ui.drawn.clear();
        // The Notes renderer notes the lines it draws (for tests): this frame's are the page's.
        self.notes_ui.drawn_lines.clear();
        let mut run: Run = Vec::new();
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if ui.available_width() >= COLUMN_TAB {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(size::SIDE_LIST);
                        page_list(ui, page, query.is_empty(), &mut run);
                    });
                    ui.separator();
                    ui.vertical(|ui| self.user_guide_scroll(ui, tab, page, &query, &mut run));
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    for p in USER_GUIDE {
                        let shown = p.id == page.id && query.is_empty();
                        if ui.selectable_label(shown, p.title).clicked() {
                            run.push(("user_guide.page", json!({ "page": p.id })));
                        }
                    }
                });
                ui.separator();
                self.user_guide_scroll(ui, tab, page, &query, &mut run);
            }
        });
        for (id, params) in run {
            // A page asked for is shown, not the search that found it.
            if matches!(id, "user_guide.page" | "help.user_guide")
                && let Some(t) = self.tabs.get_mut(&tab)
            {
                t.filter.clear();
            }
            self.run(id, &params);
        }
    }

    fn user_guide_scroll(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        page: &'static GuidePage,
        query: &str,
        run: &mut Run,
    ) {
        // Each page (and the search) scrolls on its own, so a page opens at its top.
        let salt = if query.is_empty() { page.id } else { "search" };
        egui::ScrollArea::vertical()
            .id_salt(("user-guide", tab, salt))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width().min(size::PAGE_MAX);
                ui.set_max_width(width);
                if query.is_empty() {
                    self.user_guide_page(ui, page, width, run);
                } else {
                    self.user_guide_results(ui, query, run);
                }
            });
    }

    /// A page: its lines as a Note's are shown, its screenshots, then the next page.
    fn user_guide_page(
        &mut self,
        ui: &mut Ui,
        page: &'static GuidePage,
        width: f32,
        run: &mut Run,
    ) {
        let blocks = self.user_guide_ui.blocks(page);
        for block in blocks.iter() {
            match block {
                GuideBlock::Text(lines) => {
                    for line in lines {
                        self.user_guide_ui.drawn.push(line.line.text.clone());
                        self.body_line(ui, line, width, run);
                    }
                }
                GuideBlock::Image { file, alt } => {
                    self.user_guide_ui.drawn.push(alt.clone());
                    self.user_guide_image(ui, file, alt, width);
                }
            }
        }
        ui.add_space(space::XL);
        let at = USER_GUIDE.iter().position(|p| p.id == page.id);
        if let Some(next) = at.and_then(|i| USER_GUIDE.get(i + 1)) {
            let label = format!("Next page: {}", next.title);
            if crate::components::link(ui, label).clicked() {
                run.push(("user_guide.page", json!({ "page": next.id })));
            }
        }
    }

    /// A screenshot, as wide as the page (never wider than it was taken), with its
    /// description under it.
    fn user_guide_image(&mut self, ui: &mut Ui, file: &str, alt: &str, width: f32) {
        ui.add_space(space::M);
        match self.user_guide_ui.texture(ui.ctx(), file) {
            Some(texture) => {
                let [w, h] = texture.size().map(|n| n as f32);
                let shown = width.min(w);
                ui.add(egui::Image::new((texture.id(), vec2(shown, shown * h / w))).alt_text(alt));
            }
            None => {
                ui.label(RichText::new(format!("[{alt}]")).color(theme::c().muted));
            }
        }
        ui.label(crate::components::hint_text(alt));
        ui.add_space(space::M);
    }

    /// Every page that holds each word of `query`, with the lines that hold one.
    fn user_guide_results(&mut self, ui: &mut Ui, query: &str, run: &mut Run) {
        let found = self.user_guide_ui.search(query);
        if found.is_empty() {
            let text = format!("Nothing in the User guide matches “{query}”.");
            self.user_guide_ui.drawn.push(text.clone());
            ui.label(RichText::new(text).color(theme::c().muted));
            return;
        }
        for m in found.iter() {
            self.user_guide_ui.drawn.push(m.page.title.to_string());
            if crate::components::link(ui, crate::components::subtitle_text(m.page.title)).clicked()
            {
                run.push(("user_guide.page", json!({ "page": m.page.id })));
            }
            for line in &m.lines {
                self.user_guide_ui.drawn.push(line.clone());
                ui.label(RichText::new(line).color(theme::c().muted));
            }
            ui.add_space(space::L);
        }
    }
}

/// The pages, the one shown marked; Take the tour under them.
fn page_list(ui: &mut Ui, page: &GuidePage, showing: bool, run: &mut Run) {
    ui.label(crate::components::title_text("User guide"));
    ui.add_space(space::M);
    for p in USER_GUIDE {
        let r = ui.selectable_label(showing && p.id == page.id, p.title);
        if r.clicked() {
            run.push(("user_guide.page", json!({ "page": p.id })));
        }
    }
    ui.add_space(space::XL);
    if crate::components::link(ui, "Take the tour").clicked() {
        run.push(("help.tour", json!({})));
    }
}
