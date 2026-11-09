//! The Screenshot gallery (#222): every Tab kind, toolbar pane, menu, window, dialog and
//! popover, drawn headlessly in each Look (Light, Dark, Sepia) at 1× and 2×, and compared
//! with its golden image in `docs/previews/gallery/` within a small tolerance.
//! `docs/previews/gallery.md` shows every golden on one page (`docs/dev/screenshot-gallery.md`).
//!
//! `cargo xtask gallery` runs these tests; `cargo xtask gallery --bless` (`OBS_GALLERY_BLESS`)
//! writes the goldens that changed or are new, removes the ones no surface draws any more and
//! writes the page. An image off its golden fails with what differs, and its new image and a
//! golden | new | diff image go to `target/gallery/` (CI uploads that folder).
//!
//! The fixtures are deterministic: the accessibility audit's Starter subset with the BSB's
//! Genesis 1, in a fresh launch for each surface and Look, the clock fixed at [`NOW_MS`], and
//! the pointer parked in the window's corner.

use std::path::{Path, PathBuf};

use egui::Key;
use obs_engine::TabId;

use super::a11y_audit::{Overlay, every_overlay, every_tab_kind, key, open_tab, type_text};
use super::missing_resource::{Lab, release_set, settle, setup};
use super::page_tone::{bsb_genesis_1, cross_reference_marker, open_bsb};
use super::*;

/// The fixed clock: 2026-11-09 10:30 UTC, in Unix milliseconds. Every launch's User database
/// stamps and shows times by it, local time being UTC.
const NOW_MS: i64 = 1_794_220_200_000;

/// A pixel is the same as its golden's when no channel is more than this many levels off
/// (rounding in the rasterizer and the font atlas across platforms).
const CHANNEL_TOLERANCE: u8 = 2;

/// An image matches its golden when no more than this share of its pixels differ: a few
/// anti-aliased edges, never a colour token's change (a hairline across the window is more).
const PIXEL_TOLERANCE: f64 = 0.0001;

const BLESS: &str = "OBS_GALLERY_BLESS";

/// A Look: the Theme and Page tone a gallery image is drawn in.
struct Look {
    slug: &'static str,
    title: &'static str,
    theme: &'static str,
    page_tone: &'static str,
}

const LOOKS: [Look; 3] = [
    Look {
        slug: "light",
        title: "Light",
        theme: "light",
        page_tone: "auto",
    },
    Look {
        slug: "dark",
        title: "Dark",
        theme: "dark",
        page_tone: "auto",
    },
    Look {
        slug: "sepia",
        title: "Sepia",
        theme: "light",
        page_tone: "sepia",
    },
];

/// Display scales: points to pixels.
const SCALES: [(f32, &str); 2] = [(1.0, "1x"), (2.0, "2x")];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Tabs,
    PanesMenusAndWindows,
    DialogsAndPopovers,
}

impl Section {
    const ALL: [Section; 3] = [
        Section::Tabs,
        Section::PanesMenusAndWindows,
        Section::DialogsAndPopovers,
    ];

    fn heading(self) -> &'static str {
        match self {
            Section::Tabs => "Tabs",
            Section::PanesMenusAndWindows => "Toolbar panes, menus and windows",
            Section::DialogsAndPopovers => "Dialogs and popovers",
        }
    }
}

enum Open {
    Tab(TabKind),
    Overlay(fn(&mut Harness)),
    Run(fn(&mut Harness)),
}

/// A surface of the gallery: its file name stem, its title on the page, and how to open it.
struct Surface {
    section: Section,
    slug: String,
    title: String,
    open: Open,
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A Tab kind's name on the page. No catch-all: a new Tab kind needs a name here too.
fn tab_title(kind: &TabKind) -> String {
    match kind {
        TabKind::Library => "Library".into(),
        TabKind::Resource(_) => "Resource (interlinear)".into(),
        TabKind::About(_) => "About a Resource".into(),
        TabKind::ImportReport(_) => "Import report".into(),
        TabKind::GetBibles => "Get Resources".into(),
        TabKind::StarterLibrary => "Starter library".into(),
        TabKind::Credits => "Credits".into(),
        TabKind::Home => "Home".into(),
        TabKind::KeyboardShortcuts => "Keyboard shortcuts".into(),
        TabKind::Settings => "Settings".into(),
        TabKind::Backups => "Back up & restore".into(),
        TabKind::Guide(g) => g.title().into(),
        TabKind::Tool(t) => t.title().into(),
    }
}

/// Every surface the gallery draws, in page order.
fn surfaces() -> Vec<Surface> {
    let mut out = Vec::new();
    for kind in every_tab_kind() {
        let title = tab_title(&kind);
        out.push(Surface {
            section: Section::Tabs,
            slug: format!("tab-{}", slug(&title)),
            title,
            open: Open::Tab(kind),
        });
    }
    for (name, open, _) in every_overlay() {
        let open: fn(&mut Harness) = open;
        out.push(Surface {
            section: Section::PanesMenusAndWindows,
            slug: slug(name),
            title: name.into(),
            open: Open::Overlay(open),
        });
    }
    for (name, open) in dialogs_and_popovers() {
        out.push(Surface {
            section: Section::DialogsAndPopovers,
            slug: slug(name),
            title: name.into(),
            open: Open::Run(open),
        });
    }
    out
}

// ---------------------------------------------------------------------------------------
// Dialogs and popovers the accessibility audit's overlays leave out.

fn right_click(h: &mut Harness, p: egui::Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
}

/// A word of the KJV's text in the default layout.
fn kjv_word(h: &Harness) -> egui::Pos2 {
    let kjv = h.tab_id("KJV");
    super::word_info::word_spot(h, kjv, "beginning")
}

fn selection_toolbar(h: &mut Harness) {
    let p = kjv_word(h);
    super::annotation::double_click(h, p);
}

/// The marker of the BSB's first Footnote proper (not a cross-reference note).
fn footnote_marker(h: &Harness, tab: TabId) -> egui::Pos2 {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    let m = (view.footnote_markers(900.0).into_iter())
        .find(|m| m.kind == obs_model::NoteKind::Footnote)
        .expect("a Footnote's marker on screen");
    let [x0, y0, x1, y1] = m.rect;
    egui::pos2(ox + (x0 + x1) / 2.0, oy + (y0 + y1) / 2.0)
}

/// Frames until `done` holds.
fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        h.paced_frame();
    }
    panic!("never saw {what}: {:?}", h.texts());
}

type Run = (&'static str, fn(&mut Harness));

fn dialogs_and_popovers() -> Vec<Run> {
    vec![
        ("Selection toolbar", selection_toolbar),
        ("Note editor", |h| {
            selection_toolbar(h);
            key(h, Key::N, Modifiers::COMMAND | Modifiers::SHIFT);
            h.events
                .push(Event::Text("In the beginning: before all things.".into()));
            h.frames(2);
        }),
        ("Footnote popover", |h| {
            let bsb = open_bsb(h);
            let at = footnote_marker(h, bsb);
            h.events.push(Event::PointerMoved(at));
            until(h, "the Footnote's popover", |h| {
                h.shell.st.footnotes.shown().is_some()
            });
            h.frames(10);
        }),
        ("Reference preview", |h| {
            let bsb = open_bsb(h);
            h.click(cross_reference_marker(h, bsb));
            let mut link = None;
            until(h, "the cross-reference note's link", |h| {
                h.shell
                    .st
                    .footnotes
                    .shown()
                    .is_some_and(|(_, f)| !f.link_rects.is_empty())
            });
            if let Some((_, f)) = h.shell.st.footnotes.shown() {
                link = f.link_rects.first().map(|r| r.center());
            }
            h.events.push(Event::PointerMoved(link.unwrap()));
            until(h, "the reference preview", |h| h.find("Gen 1:3").is_some());
            h.frames(10);
        }),
        ("Tab switcher", |h| {
            for id in ["tool.search", "tool.history"] {
                h.shell.run(id, &json!({}));
            }
            settle_all(h);
            let ctrl = Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            };
            h.modifiers = ctrl;
            for pressed in [true, false] {
                h.events.push(Event::Key {
                    key: Key::Tab,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: ctrl,
                });
            }
            h.frames(3);
        }),
        ("Find bar", |h| {
            key(h, Key::F, Modifiers::COMMAND);
            type_text(h, "light");
            settle_all(h);
        }),
        ("Move Note", |h| {
            let kjv = h.tab_id("KJV");
            h.shell.st.confirm_reanchor(
                "n".into(),
                "Genesis 1:1 (KJV)".into(),
                kjv,
                json!({}),
                "Genesis 1:2 (KJV)".into(),
            );
            h.frames(3);
        }),
        ("Delete Notebook", |h| {
            let study = h
                .shell
                .run("notebook.create", &json!({ "name": "Study" }))
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            h.shell.run(
                "annotation.note",
                &json!({ "passage": "Gen 1:1", "body": "Beginnings", "notebook": study }),
            );
            h.shell.run("tool.notes", &json!({ "split": true }));
            settle(h);
            settle_all(h);
            let p = h.at("Study");
            right_click(h, p);
            h.click_text("Delete…");
            assert!(h.shell.st.notes_ui.deleting.is_some(), "{:?}", h.texts());
        }),
        ("Remove a Resource", |h| {
            h.shell.run("library.remove", &json!({ "id": "mhc" }));
            h.frames(3);
            assert!(h.shell.st.confirm.is_some());
        }),
        ("Save layout as", |h| {
            h.shell.run("layout.save", &json!({}));
            h.frames(3);
            type_text(h, "Study desk");
        }),
        ("Import a Resource", |h| {
            h.shell.run("library.import", &json!({}));
            h.frames(3);
            assert!(h.shell.st.pick_import);
        }),
        ("Visual filters", |h| {
            h.shell.run("view.visual_filters", &json!({}));
            h.frames(3);
        }),
        ("Language chooser", |h| {
            h.shell
                .run("library.get_resources", &json!({ "split": true }));
            settle(h);
            settle_all(h);
            h.click_text("All languages");
            // Off the button, so its tooltip is not drawn over the list.
            h.events
                .push(Event::PointerMoved(egui::pos2(1395.0, 895.0)));
            h.frames(3);
            assert!(
                h.find("Search languages: name or code").is_some(),
                "{:?}",
                h.texts()
            );
        }),
        ("Page tone hint", |h| {
            // The hint offers Auto when the theme turns dark over a light Page tone the user
            // chose: the Look's own (Sepia) or else Paper. It stays open, back in the Look's
            // theme, until answered.
            let dark = h.ctx.theme() == egui::Theme::Dark;
            let tone = match h.shell.session().appearance.page_tone {
                obs_engine::PageTone::Auto => obs_engine::PageTone::Paper,
                chosen => chosen,
            };
            h.shell
                .run("view.page_tone", &json!({ "value": tone.id() }));
            for theme in ["light", "dark", if dark { "dark" } else { "light" }] {
                h.shell.run("view.theme", &json!({ "value": theme }));
                h.frames(2);
            }
            let hint = format!(
                "Page tone is set to {}. Use Auto to follow the theme?",
                tone.label()
            );
            assert!(h.find(&hint).is_some(), "{:?}", h.texts());
            // Past its fade-in.
            h.frames(15);
        }),
        ("About Open Bible", |h| {
            h.shell.run("help.about", &json!({}));
            h.frames(3);
        }),
    ]
}

// ---------------------------------------------------------------------------------------
// Drawing.

/// A fresh launch with the gallery's fixtures in `look`.
fn launch(name: &str, look: &Look) -> (Harness, Lab) {
    let mut set = release_set();
    set.push(obs_engine::testing::topical_index("nave"));
    let tipnr = obs_engine::testing::tipnr();
    set.push(obs_engine::testing::theographic(Some(&tipnr)));
    set.push(bsb_genesis_1());
    obs_store::clock::fix(Some(NOW_MS));
    let (mut h, lab) = setup(&format!("gallery-{name}"), &set);
    // The launch's backup is dated by its file's time.
    let taken = std::time::UNIX_EPOCH + std::time::Duration::from_millis(NOW_MS as u64);
    for entry in std::fs::read_dir(lab.root.join("backups")).unwrap() {
        let file = std::fs::File::options()
            .write(true)
            .open(entry.unwrap().path())
            .unwrap();
        file.set_modified(taken).unwrap();
    }
    // Some overlays are reached with Tab, as the audit does.
    h.ctx.enable_accesskit();
    h.shell
        .run("view.page_tone", &json!({ "value": look.page_tone }));
    h.shell.run("view.theme", &json!({ "value": look.theme }));
    // Out of the way of every surface's hover effects.
    h.events
        .push(Event::PointerMoved(egui::pos2(1395.0, 895.0)));
    settle(&mut h);
    (h, lab)
}

fn open(h: &mut Harness, lab: &Lab, how: &Open) {
    match how {
        // Its report shows the folder imported: one the same on every computer.
        Open::Tab(TabKind::ImportReport(_)) => {
            let source = root().join("target/gallery/usfm");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("GEN.usfm"),
                "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning.\n\\ip Dropped.\n",
            )
            .unwrap();
            // Tests run in the crate's folder.
            h.shell.run(
                "import.start",
                &json!({ "path": "../../target/gallery/usfm" }),
            );
            settle(h);
            settle_all(h);
        }
        // With something in them: a Note card, and a Favorite.
        Open::Tab(TabKind::Tool(Tool::Notes)) => {
            h.shell.run(
                "annotation.note",
                &json!({ "passage": "Gen 1:1", "body": "**Beginnings**: before all things." }),
            );
            h.shell.run("tool.notes", &json!({ "split": true }));
            settle(h);
            settle_all(h);
        }
        Open::Tab(TabKind::Tool(Tool::Favorites)) => {
            let kjv = h.tab_id("KJV");
            h.shell.run("favorite.add", &json!({ "tab": kjv.0 }));
            h.shell.run("tool.favorites", &json!({ "split": true }));
            settle(h);
            settle_all(h);
        }
        Open::Tab(kind) => open_tab(h, lab, kind),
        Open::Overlay(f) | Open::Run(f) => f(h),
    }
    h.frames(10);
}

struct Image {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn goldens() -> PathBuf {
    root().join("docs/previews/gallery")
}

fn out_dir() -> PathBuf {
    root().join("target/gallery")
}

fn image_name(surface: &Surface, look: &Look, scale: &str) -> String {
    format!("{}-{}-{scale}", surface.slug, look.slug)
}

/// Goldens are compressed hard (they are committed); failure output fast.
fn write_png(path: &Path, image: &Image, compression: png::Compression) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        image.width as u32,
        image.height as u32,
    );
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(compression);
    enc.write_header()
        .unwrap()
        .write_image_data(&image.rgb)
        .unwrap();
}

fn read_png(path: &Path) -> Option<Image> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let channels = info.color_type.samples();
    let rgb = buf
        .chunks(channels)
        .flat_map(|p| match channels {
            1 | 2 => [p[0], p[0], p[0]],
            _ => [p[0], p[1], p[2]],
        })
        .collect();
    Some(Image {
        width: info.width as usize,
        height: info.height as usize,
        rgb,
    })
}

/// How an image differs from its golden, beyond the tolerance.
#[derive(Debug, PartialEq)]
enum Mismatch {
    Size {
        golden: (usize, usize),
        new: (usize, usize),
    },
    Pixels {
        count: usize,
        of: usize,
        /// The largest channel difference.
        most: u8,
        /// Where they are: left, top, right, bottom (exclusive), in pixels.
        bounds: [usize; 4],
    },
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mismatch::Size { golden, new } => write!(
                f,
                "is {}×{} pixels, its golden {}×{}",
                new.0, new.1, golden.0, golden.1
            ),
            Mismatch::Pixels {
                count,
                of,
                most,
                bounds: [x0, y0, x1, y1],
            } => write!(
                f,
                "{count} of {of} pixels differ by more than {CHANNEL_TOLERANCE} levels (up to \
                 {most}), within x {x0}..{x1}, y {y0}..{y1}"
            ),
        }
    }
}

/// Whether a channel difference is past [`CHANNEL_TOLERANCE`], pixel by pixel.
fn differing(golden: &Image, new: &Image) -> Vec<Option<u8>> {
    (golden.rgb.chunks(3).zip(new.rgb.chunks(3)))
        .map(|(a, b)| {
            let most = (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
            (most > CHANNEL_TOLERANCE).then_some(most)
        })
        .collect()
}

/// `new` against its `golden`: `None` within the tolerance.
fn compare(golden: &Image, new: &Image) -> Option<Mismatch> {
    if (golden.width, golden.height) != (new.width, new.height) {
        return Some(Mismatch::Size {
            golden: (golden.width, golden.height),
            new: (new.width, new.height),
        });
    }
    let of = new.width * new.height;
    let mut count = 0;
    let mut most = 0;
    let mut bounds = [usize::MAX, usize::MAX, 0, 0];
    for (i, d) in differing(golden, new).into_iter().enumerate() {
        let Some(d) = d else { continue };
        let (x, y) = (i % new.width, i / new.width);
        count += 1;
        most = most.max(d);
        bounds = [
            bounds[0].min(x),
            bounds[1].min(y),
            bounds[2].max(x + 1),
            bounds[3].max(y + 1),
        ];
    }
    (count as f64 > of as f64 * PIXEL_TOLERANCE).then_some(Mismatch::Pixels {
        count,
        of,
        most,
        bounds,
    })
}

/// Golden | new | diff side by side; the diff is the golden faded to grey with each pixel
/// past the tolerance in magenta. Images of different sizes are shown on a blank canvas.
fn diff_image(golden: &Image, new: &Image) -> Image {
    let (w, h) = (golden.width.max(new.width), golden.height.max(new.height));
    let mut rgb = vec![255u8; w * 3 * h * 3];
    let mut put = |panel: usize, x: usize, y: usize, p: [u8; 3]| {
        let at = (y * w * 3 + panel * w + x) * 3;
        rgb[at..at + 3].copy_from_slice(&p);
    };
    let px = |img: &Image, x: usize, y: usize| {
        let at = (y * img.width + x) * 3;
        [img.rgb[at], img.rgb[at + 1], img.rgb[at + 2]]
    };
    for (panel, img) in [(0, golden), (1, new)] {
        for y in 0..img.height {
            for x in 0..img.width {
                put(panel, x, y, px(img, x, y));
            }
        }
    }
    let same_size = (golden.width, golden.height) == (new.width, new.height);
    let differ = if same_size {
        differing(golden, new)
    } else {
        Vec::new()
    };
    for y in 0..golden.height {
        for x in 0..golden.width {
            let [r, g, b] = px(golden, x, y);
            let grey = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
            let faded = 255 - (255 - grey) / 4;
            let p = match differ.get(y * golden.width + x) {
                Some(Some(_)) => [255, 0, 255],
                _ => [faded; 3],
            };
            put(2, x, y, p);
        }
    }
    Image {
        width: w * 3,
        height: h,
        rgb,
    }
}

fn blessing() -> bool {
    std::env::var_os(BLESS).is_some()
}

fn relative(path: &Path) -> String {
    let root = root().canonicalize().unwrap();
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(&root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `new` against its golden `name`: blessed, or a failure written to `target/gallery/`.
fn check(name: &str, new: Image) -> Option<String> {
    let golden_path = goldens().join(format!("{name}.png"));
    let golden = read_png(&golden_path);
    let mismatch = match &golden {
        None => None,
        Some(g) => compare(g, &new),
    };
    if golden.is_some() && mismatch.is_none() {
        return None;
    }
    if blessing() {
        write_png(&golden_path, &new, png::Compression::High);
        eprintln!("gallery: blessed {}", relative(&golden_path));
        return None;
    }
    let new_path = out_dir().join("new").join(format!("{name}.png"));
    write_png(&new_path, &new, png::Compression::Fast);
    let Some(golden) = golden else {
        return Some(format!(
            "{name}: no golden at {} (new image: {})",
            relative(&golden_path),
            relative(&new_path)
        ));
    };
    let diff_path = out_dir().join("diff").join(format!("{name}.png"));
    write_png(
        &diff_path,
        &diff_image(&golden, &new),
        png::Compression::Fast,
    );
    Some(format!(
        "{name}: {}\n    golden: {}\n    new:    {}\n    diff:   {} (golden | new | diff)",
        mismatch.unwrap(),
        relative(&golden_path),
        relative(&new_path),
        relative(&diff_path)
    ))
}

/// Draw every surface of `section` in each Look and scale and check it against its golden.
fn gallery(section: Section) {
    let mut failures = Vec::new();
    let mut checked = 0;
    for surface in surfaces().iter().filter(|s| s.section == section) {
        for look in &LOOKS {
            let (mut h, lab) = launch(&format!("{}-{}", surface.slug, look.slug), look);
            open(&mut h, &lab, &surface.open);
            for (scale, scale_slug) in SCALES {
                h.root_info.native_pixels_per_point = Some(scale);
                h.frames(4);
                let (width, height, rgb) = h.pixels();
                let name = image_name(surface, look, scale_slug);
                checked += 1;
                failures.extend(check(&name, Image { width, height, rgb }));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} {} images differ from their goldens:\n\n{}\n\nIf the change is \
         intended, run `cargo xtask gallery --bless` and commit docs/previews/gallery/.",
        failures.len(),
        section.heading(),
        failures.join("\n\n")
    );
}

#[test]
fn gallery_tabs() {
    gallery(Section::Tabs);
}

#[test]
fn gallery_panes_menus_and_windows() {
    gallery(Section::PanesMenusAndWindows);
}

#[test]
fn gallery_dialogs_and_popovers() {
    gallery(Section::DialogsAndPopovers);
}

// ---------------------------------------------------------------------------------------
// The page.

/// `docs/previews/gallery.md`: every surface's goldens, 1× shown, 2× linked.
fn page() -> String {
    let surfaces = surfaces();
    let mut md = String::from(
        "# Screenshot gallery\n\n\
         <!-- Written by `cargo xtask gallery --bless` (crates/ui-egui/src/gallery_tests.rs); \
         do not edit. -->\n\n\
         Every Tab kind, toolbar pane, menu, window, dialog and popover in each Look at 1× \
         (2× linked). These are the goldens CI compares the app against; see \
         [docs/dev/screenshot-gallery.md](../dev/screenshot-gallery.md).\n",
    );
    for section in Section::ALL {
        md.push_str(&format!("\n## {}\n", section.heading()));
        for s in surfaces.iter().filter(|s| s.section == section) {
            md.push_str(&format!("\n### {}\n\n|", s.title));
            for look in &LOOKS {
                md.push_str(&format!(" {} |", look.title));
            }
            md.push_str("\n|");
            md.push_str(&"---|".repeat(LOOKS.len()));
            md.push_str("\n|");
            for look in &LOOKS {
                let name = image_name(s, look, "1x");
                md.push_str(&format!(
                    " ![{}, {}](gallery/{name}.png) |",
                    s.title, look.title
                ));
            }
            md.push_str("\n|");
            for look in &LOOKS {
                let name = image_name(s, look, "2x");
                md.push_str(&format!(" [2×](gallery/{name}.png) |"));
            }
            md.push('\n');
        }
    }
    md
}

fn every_image_name() -> Vec<String> {
    let mut names = Vec::new();
    for s in surfaces() {
        for look in &LOOKS {
            for (_, scale) in SCALES {
                names.push(image_name(&s, look, scale));
            }
        }
    }
    names
}

/// The page lists every golden, and there is no golden of a surface the gallery no longer
/// draws; blessing writes the page and removes those goldens.
#[test]
fn gallery_page_shows_every_golden() {
    let names = every_image_name();
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "two surfaces share a name");
    let page_path = root().join("docs/previews/gallery.md");
    let mut stale = Vec::new();
    for entry in std::fs::read_dir(goldens()).into_iter().flatten() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if !unique.contains(&stem) {
            stale.push(path);
        }
    }
    if blessing() {
        std::fs::write(&page_path, page()).unwrap();
        for path in stale {
            std::fs::remove_file(&path).unwrap();
            eprintln!("gallery: removed {}", path.display());
        }
        return;
    }
    let written = std::fs::read_to_string(&page_path).unwrap_or_default();
    assert!(
        written == page(),
        "docs/previews/gallery.md is out of date: run `cargo xtask gallery --bless`"
    );
    assert!(
        stale.is_empty(),
        "goldens of no surface (run `cargo xtask gallery --bless`): {stale:?}"
    );
}

/// Each surface opens in its own launch: what one leaves behind is not in the next.
#[test]
fn every_tab_kind_and_overlay_is_in_the_gallery() {
    let s = surfaces();
    let tabs = s.iter().filter(|s| s.section == Section::Tabs).count();
    let overlays: Vec<Overlay> = every_overlay();
    assert_eq!(tabs, every_tab_kind().len());
    assert_eq!(
        s.iter()
            .filter(|s| s.section == Section::PanesMenusAndWindows)
            .count(),
        overlays.len()
    );
}

// ---------------------------------------------------------------------------------------
// The comparison.

fn flat(width: usize, height: usize, p: [u8; 3]) -> Image {
    Image {
        width,
        height,
        rgb: p.repeat(width * height),
    }
}

#[test]
fn rounding_within_the_tolerance_matches() {
    let golden = flat(100, 100, [200, 200, 200]);
    let mut new = flat(100, 100, [202, 199, 200]);
    assert_eq!(compare(&golden, &new), None, "2 levels off everywhere");
    // One pixel in 10,000 may be further off: an anti-aliased edge.
    new.rgb[..3].copy_from_slice(&[0, 0, 0]);
    assert_eq!(compare(&golden, &new), None);
    new.rgb[3..6].copy_from_slice(&[0, 0, 0]);
    assert!(compare(&golden, &new).is_some(), "two are too many");
}

/// Acceptance (#222): a colour token's change — here a fill 8 levels redder along a band, as
/// `LIGHT.shell`'s red would be — fails with where and how much, and a diff marking it.
#[test]
fn a_changed_colour_fails_with_a_readable_diff() {
    let golden = flat(40, 30, [232, 236, 235]);
    let mut new = flat(40, 30, [232, 236, 235]);
    for y in 10..20 {
        for x in 5..35 {
            let at = (y * 40 + x) * 3;
            new.rgb[at] = 232 ^ 8;
        }
    }
    let mismatch = compare(&golden, &new).expect("a mismatch");
    assert_eq!(
        mismatch.to_string(),
        "300 of 1200 pixels differ by more than 2 levels (up to 8), within x 5..35, y 10..20"
    );
    let diff = diff_image(&golden, &new);
    assert_eq!((diff.width, diff.height), (120, 30));
    let at = |x: usize, y: usize| {
        let i = (y * 120 + x) * 3;
        [diff.rgb[i], diff.rgb[i + 1], diff.rgb[i + 2]]
    };
    assert_eq!(at(5, 10), [232, 236, 235], "the golden on the left");
    assert_eq!(at(45, 10), [224, 236, 235], "the new image in the middle");
    assert_eq!(at(85, 10), [255, 0, 255], "the change marked on the right");
    assert_ne!(at(80, 0), [255, 0, 255], "nothing else marked");
    assert_eq!(
        compare(&golden, &flat(41, 30, [0; 3])).unwrap().to_string(),
        "is 41×30 pixels, its golden 40×30"
    );
}

#[test]
fn a_png_reads_back_as_written() {
    let dir = std::env::temp_dir().join(format!("obs-gallery-png-{}", std::process::id()));
    let mut image = flat(3, 2, [1, 2, 3]);
    image.rgb[15..18].copy_from_slice(&[250, 128, 7]);
    let path = dir.join("a.png");
    write_png(&path, &image, png::Compression::Fast);
    let back = read_png(&path).unwrap();
    assert_eq!((back.width, back.height), (3, 2));
    assert_eq!(back.rgb, image.rgb);
    let _ = std::fs::remove_dir_all(&dir);
}
