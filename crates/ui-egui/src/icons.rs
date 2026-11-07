//! The interface's icons (#220): one SVG line set (Lucide, ISC; `assets/icons`, credited in
//! `ATTRIBUTION.toml`) in one stroke weight, rasterized by resvg for each size and display
//! scale they are drawn at (ADR 0012) and tinted with a colour role of the chrome theme. No
//! emoji or ad-hoc symbol stands in for an icon anywhere in the interface; a test checks.
//!
//! An icon is a white coverage mask, so one texture serves every colour. It is rasterized at
//! its size in physical pixels (16 points at 1.5× is 24 pixels) and sampled texel for pixel,
//! so it is as crisp at 1×, 1.5× and 2× as the drawing allows; a new scale (a moved window, a
//! changed Interface size) rasterizes it again.

use std::collections::HashMap;

use egui::{Color32, ColorImage, Context, Image, Painter, Rect, TextureHandle, Vec2, vec2};

/// Beside small text (the chrome's 11.5 to 12.5 point labels).
pub const SMALL: f32 = 12.0;
/// Beside body text (14 points): menu items, buttons, Tab header controls.
pub const BODY: f32 = 16.0;
/// Toolbar Launchers.
pub const LARGE: f32 = 18.0;

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        /// One icon of the set, named for what it means here.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            /// Every icon, in catalogue order.
            pub const ALL: &[Icon] = &[$(Icon::$variant,)*];

            /// Its SVG file under `assets/icons/`, without `.svg`.
            pub fn file(self) -> &'static str {
                match self {
                    $(Icon::$variant => $file,)*
                }
            }

            fn svg(self) -> &'static str {
                match self {
                    $(Icon::$variant => include_str!(concat!("../assets/icons/", $file, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    // Controls.
    Close => "x",
    Check => "check",
    CheckCircle => "circle-check",
    Add => "plus",
    Remove => "minus",
    More => "ellipsis",
    Overflow => "chevrons-right",
    Grip => "grip-vertical",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    ChevronDown => "chevron-down",
    Back => "arrow-left",
    Forward => "arrow-right",
    MoveUp => "arrow-up",
    MoveDown => "arrow-down",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    Star => "star",
    StarFilled => "star-filled",
    Pin => "pin",
    Note => "pencil",
    Search => "search",
    Shown => "eye",
    Hidden => "eye-off",
    Warning => "triangle-alert",
    Swatch => "square-filled",
    Contents => "table-of-contents",
    Home => "house",
    Copy => "copy",
    // Launchers.
    Library => "library",
    Guides => "compass",
    Tools => "layout-grid",
    Layouts => "layout-panel-left",
    Import => "file-up",
    Download => "download",
    Starter => "package",
    // Guides.
    PassageGuide => "book-open",
    ExegeticalGuide => "microscope",
    WordStudy => "whole-word",
    Factbook => "book-user",
    TopicGuide => "tags",
    // Tools.
    TextComparison => "columns-3",
    Notes => "notebook-pen",
    WordInfo => "info",
    CrossReferences => "link",
    History => "history",
    ReadingPlans => "calendar-check",
    Atlas => "map",
    Timeline => "chart-gantt",
    // Guide sections.
    Commentaries => "message-square-text",
    KeyWords => "key-round",
    People => "users",
    Lexicon => "book-a",
    TranslatedAs => "languages",
    Occurrences => "chart-column",
    Forms => "list-tree",
    Summary => "id-card",
    KeyPassages => "bookmark",
    Articles => "book-text",
    Related => "network",
    GrammaticalNotes => "list-checks",
    TopicalEntries => "list",
    LibrarySearch => "text-search",
}

impl Icon {
    /// What a test sees where this icon is drawn (the harness lists it with painted text).
    #[cfg(test)]
    pub fn marker(self) -> String {
        format!("[{}]", self.file())
    }
}

/// The icon of a menu command, where the menus and Launcher panes show one.
pub fn for_command(id: &str) -> Option<Icon> {
    Some(match id {
        "library.open" => Icon::Library,
        "library.import" => Icon::Import,
        "library.get_resources" | "library.get_bibles" => Icon::Download,
        "library.starter" => Icon::Starter,
        "guide.passage_guide" => Icon::PassageGuide,
        "guide.exegetical_guide" => Icon::ExegeticalGuide,
        "guide.word_study" => Icon::WordStudy,
        "guide.factbook" => Icon::Factbook,
        "guide.topic_guide" => Icon::TopicGuide,
        "tool.search" => Icon::Search,
        "tool.text_comparison" => Icon::TextComparison,
        "tool.notes" => Icon::Notes,
        "tool.word_info" => Icon::WordInfo,
        "tool.cross_references" => Icon::CrossReferences,
        "tool.favorites" => Icon::Star,
        "tool.history" => Icon::History,
        "tool.reading_plans" => Icon::ReadingPlans,
        "tool.atlas" => Icon::Atlas,
        "tool.timeline" => Icon::Timeline,
        "tool.copy_verses" => Icon::Copy,
        "home.open" => Icon::Home,
        _ => return None,
    })
}

/// The icon of a Guide section's header.
pub fn for_section(section: obs_engine::GuideSection) -> Icon {
    use obs_engine::GuideSection as S;
    match section {
        S::Commentaries => Icon::Commentaries,
        S::CrossReferences => Icon::CrossReferences,
        S::KeyWords => Icon::KeyWords,
        S::PeopleAndPlaces => Icon::People,
        S::Related => Icon::Related,
        S::LexiconEntries | S::LexiconSummaries => Icon::Lexicon,
        S::TranslatedAs => Icon::TranslatedAs,
        S::Occurrences => Icon::Occurrences,
        S::Forms => Icon::Forms,
        S::Summary => Icon::Summary,
        S::KeyPassages | S::TopicPassages => Icon::KeyPassages,
        S::DictionaryArticles | S::TopicArticles => Icon::Articles,
        S::Map => Icon::Atlas,
        S::WordByWord => Icon::WordStudy,
        S::GrammaticalNotes => Icon::GrammaticalNotes,
        S::TopicalEntries => Icon::TopicalEntries,
        S::TopicSearch => Icon::LibrarySearch,
    }
}

/// The secondary colour of an icon on a Resource page (a Note card, a Guide row) rather than
/// on the chrome: the page's verse-number colour, which reads on Paper, Sepia and Dark.
pub fn page_muted(page: &crate::Palette) -> Color32 {
    page.verse_number
}

/// `icon` as a coverage mask `px` pixels square: white, with the drawing's coverage as alpha
/// (premultiplied). Blank if the SVG cannot be read, which a test rules out for the set.
pub fn rasterize(icon: Icon, px: u32) -> ColorImage {
    let px = px.max(1);
    let side = px as usize;
    let mut out = ColorImage::filled([side, side], Color32::TRANSPARENT);
    let Ok(tree) = resvg::usvg::Tree::from_str(icon.svg(), &resvg::usvg::Options::default()) else {
        return out;
    };
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(px, px) else {
        return out;
    };
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    for (dst, src) in out.pixels.iter_mut().zip(pixmap.pixels()) {
        let a = src.alpha();
        *dst = Color32::from_rgba_premultiplied(a, a, a, a);
    }
    out
}

/// The textures made so far, by icon and size in physical pixels.
#[derive(Clone, Default)]
struct Cache(HashMap<(Icon, u32), TextureHandle>);

fn cache_id() -> egui::Id {
    egui::Id::unique("obs-icon-textures")
}

/// How many physical pixels square `icon` is at `size` points on `ctx` now.
pub fn pixels(ctx: &Context, size: f32) -> u32 {
    (size * ctx.pixels_per_point()).round().max(1.0) as u32
}

/// The texture of `icon` at `size` points for the display scale of the viewport being drawn,
/// rasterized on first use.
pub fn texture(ctx: &Context, icon: Icon, size: f32) -> egui::load::SizedTexture {
    let px = pixels(ctx, size);
    // Borrowed in place: `get_temp` would clone every handle in the cache on each lookup.
    let cached = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .0
            .get(&(icon, px))
            .map(TextureHandle::id)
    });
    let id = cached.unwrap_or_else(|| {
        let handle = ctx.load_texture(
            format!("icon-{}-{px}", icon.file()),
            rasterize(icon, px),
            // Drawn texel for pixel: no filtering softens the edges.
            egui::TextureOptions::NEAREST,
        );
        let id = handle.id();
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(cache_id())
                .0
                .insert((icon, px), handle);
        });
        id
    });
    egui::load::SizedTexture::new(id, Vec2::splat(size))
}

/// `icon` as an image widget (or a Button's, a menu item's or a label's leading atom),
/// `size` points square, in `color`.
pub fn image(ctx: &Context, icon: Icon, size: f32, color: Color32) -> Image<'static> {
    Image::from_texture(texture(ctx, icon, size))
        .fit_to_exact_size(vec2(size, size))
        .tint(color)
}

/// `icon` in the chrome's muted ink, then `text`: the atoms of a menu item, Button or label
/// with a leading icon.
pub fn with_icon(
    ui: &egui::Ui,
    icon: Icon,
    text: impl Into<egui::WidgetText>,
) -> (Image<'static>, egui::WidgetText) {
    (
        image(ui.ctx(), icon, BODY, crate::theme::c().muted),
        text.into(),
    )
}

/// Tell assistive technology whether a toggle menu item (Red letter, Auto-hide) is on. Its
/// check is an image a screen reader does not read, so without this both states would be
/// announced alike.
pub fn announce_toggled(response: &egui::Response, on: bool) {
    use egui::accesskit::Toggled;
    response.ctx.accesskit_node_builder(response.id, |n| {
        n.set_toggled(if on { Toggled::True } else { Toggled::False });
    });
}

/// An icon's room left blank, so a menu item without an icon lines up with those that have
/// one.
pub fn blank() -> egui::Atom<'static> {
    egui::Atom::paint(Vec2::splat(BODY), |_, _| {})
}

/// A label after `icon` in `color`, on one line.
pub fn label(
    ui: &mut egui::Ui,
    icon: Icon,
    color: Color32,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add(image(ui.ctx(), icon, BODY, color));
        ui.label(text)
    })
    .inner
}

/// The Close button of a removable chip (a Search scope's Bible, a filter), named and
/// explained by `what` ("Do not search this Bible").
pub fn chip_close(ui: &mut egui::Ui, what: &str) -> egui::Response {
    let img = image(ui.ctx(), Icon::Close, SMALL, crate::theme::c().muted);
    ui.add(
        egui::Button::new(img)
            .frame(false)
            .min_size(vec2(16.0, 20.0)),
    )
    .accessible_name(what)
    .on_hover_text(what)
}

/// A small framed button showing only `icon` (Zoom in, Zoom out), named `name` for screen
/// readers and its tooltip.
pub fn small_button(ui: &mut egui::Ui, icon: Icon, name: &str) -> egui::Response {
    let img = image(ui.ctx(), icon, SMALL, crate::theme::c().ink);
    ui.add(egui::Button::new(img).small())
        .accessible_name(name)
        .on_hover_text(name)
}

/// A small button that goes on to `text` (a Factbook, a Word Study): its label, then a
/// chevron.
pub fn onward(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let chevron = image(ui.ctx(), Icon::ChevronRight, SMALL, crate::theme::c().muted);
    ui.add(egui::Button::new((
        egui::RichText::new(text).small(),
        chevron,
    )))
}

/// Paint `icon` `size` points square, centred in `rect` on the pixel grid, in `color`.
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, size: f32, color: Color32) {
    let ctx = painter.ctx();
    let tex = texture(ctx, icon, size);
    let ppp = ctx.pixels_per_point();
    let min = rect.center() - Vec2::splat(size / 2.0);
    let snapped = egui::pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
    painter.image(
        tex.id,
        Rect::from_min_size(snapped, Vec2::splat(size)),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
}

/// A frameless button showing only `icon`, named `name` for screen readers (callers add the
/// tooltip, which may name a shortcut).
pub fn button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    name: &str,
) -> egui::Response {
    let img = image(ui.ctx(), icon, size, color);
    ui.add(egui::Button::new(img).frame(false))
        .accessible_name(name)
}

/// Which icon (if any) a texture is, for tests that find icons where they are drawn.
#[cfg(test)]
pub(crate) fn icon_of(ctx: &Context, tex: egui::TextureId) -> Option<Icon> {
    ctx.data_mut(|d| {
        (d.get_temp_mut_or_default::<Cache>(cache_id()).0.iter())
            .find(|(_, h)| h.id() == tex)
            .map(|((icon, _), _)| *icon)
    })
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
