//! The Screenshot gallery (#222): every Tab kind, toolbar pane, menu, window, dialog, popover
//! and chooser, drawn headlessly in each Look (Light, Dark, Sepia) at 1× and 2×, and compared
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
use obs_engine::{Launcher, TabId};
use serde_json::Value;

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
pub(super) struct Look {
    slug: &'static str,
    title: &'static str,
    theme: &'static str,
    page_tone: &'static str,
}

pub(super) const LOOKS: [Look; 3] = [
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
    MenusAndChoosers,
    /// Each surface's empty, loading and error states (#223).
    States,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Tabs,
        Section::PanesMenusAndWindows,
        Section::DialogsAndPopovers,
        Section::MenusAndChoosers,
        Section::States,
    ];

    fn heading(self) -> &'static str {
        match self {
            Section::Tabs => "Tabs",
            Section::PanesMenusAndWindows => "Toolbar panes, menus and windows",
            Section::DialogsAndPopovers => "Dialogs and popovers",
            Section::MenusAndChoosers => "Menus and choosers in Tabs and windows",
            Section::States => "Empty, loading and error states",
        }
    }
}

enum Open {
    Tab(TabKind),
    /// What opens it, and what shows it is open.
    Run(fn(&mut Harness), fn(&Harness) -> bool),
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
        TabKind::UserGuide => "User guide".into(),
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
    let lists = [
        (Section::Tabs, super::library_rows::library_rows()),
        (Section::Tabs, super::library_wording::library_wording()),
        (Section::PanesMenusAndWindows, every_overlay()),
        (Section::PanesMenusAndWindows, menus_and_toolbar()),
        (Section::DialogsAndPopovers, dialogs_and_popovers()),
        (Section::MenusAndChoosers, menus_and_choosers()),
        (Section::States, super::pane_states::pane_states()),
    ];
    for (section, list) in lists {
        for (name, open, is_open) in list {
            out.push(Surface {
                section,
                slug: slug(name),
                title: name.into(),
                open: Open::Run(open, is_open),
            });
        }
    }
    out
}

/// Whether `surface` is on screen: its Tab focused, or what shows it is open.
fn is_open(h: &Harness, surface: &Surface) -> bool {
    match &surface.open {
        Open::Tab(kind) => {
            let ws = &h.shell.session().workspace;
            let focused = ws.focused().and_then(|t| ws.tab(t)).map(|t| &t.kind);
            match (kind, focused) {
                (TabKind::ImportReport(_), Some(TabKind::ImportReport(_))) => true,
                (kind, focused) => focused == Some(kind),
            }
        }
        Open::Run(_, is_open) => is_open(h),
    }
}

// ---------------------------------------------------------------------------------------
// Opening menus, popovers and choosers.

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

/// The pointer on `text` (a menu's item: its submenu opens).
fn hover(h: &mut Harness, text: &str) {
    let p = h.at(text);
    h.events.push(Event::PointerMoved(p));
    h.frames(4);
}

/// Where `text` is painted within the part of the window `keep` holds, the topmost first.
fn spot(h: &Harness, text: &str, keep: impl Fn(egui::Rect) -> bool) -> egui::Pos2 {
    (h.shapes.iter())
        .filter(|(r, t)| t == text && keep(*r))
        .map(|(r, _)| *r)
        .min_by(|a, b| {
            (a.top(), a.left())
                .partial_cmp(&(b.top(), b.left()))
                .unwrap()
        })
        .unwrap_or_else(|| panic!("{text:?} not on screen there: {:?}", h.texts()))
        .center()
}

fn first(h: &Harness, text: &str) -> egui::Pos2 {
    spot(h, text, |_| true)
}

fn popup(h: &Harness) -> bool {
    egui::Popup::is_any_open(&h.ctx)
}

fn shows(h: &Harness, text: &str) -> bool {
    h.find(text).is_some()
}

fn shows_start(h: &Harness, start: &str) -> bool {
    h.texts().iter().any(|t| t.starts_with(start))
}

/// A menu item of command `id` is drawn (its label, maybe with a count after it).
fn command_shown(h: &Harness, id: &str) -> bool {
    let commands = h.shell.st.session.commands();
    let label = commands.get(id).expect("a command").label;
    shows_start(h, label)
}

/// Each option of the View menu's choice `command` is drawn.
fn choice_shown(h: &Harness, command: &str) -> bool {
    let (options, _) = obs_engine::choices(&h.shell.st.session, command);
    options.iter().all(|(_, label)| shows(h, label))
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

/// "beginning" in Genesis 1:1 highlighted in the palette's first style.
fn highlight_a_word(h: &mut Harness) {
    selection_toolbar(h);
    key(h, Key::Num1, Modifiers::ALT);
    settle_all(h);
}

fn note_editor(h: &mut Harness) {
    selection_toolbar(h);
    key(h, Key::N, Modifiers::COMMAND | Modifiers::SHIFT);
    h.events
        .push(Event::Text("In the beginning: before all things.".into()));
    h.frames(2);
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

/// This is version 1.2.3, and the release source (a mock: nothing reaches the network) has
/// published `tag`, with release notes.
fn offer_updates(h: &mut Harness, tag: &str) {
    use obs_engine::testing::{MockRelease, MockReleases};
    let releases = std::sync::Arc::new(MockReleases::new());
    releases.publish(&[MockRelease::with_archive(
        tag,
        "## What's new\n\nSearch is faster.\n\n- Notes are kept when a Resource is updated\n\
         - A crash on start is fixed",
        b"new app",
    )]);
    let s = &mut h.shell.st.session;
    s.set_fetch(releases);
    s.set_update_source(MockReleases::source());
    s.set_current_version(obs_engine::updates::Version::parse("1.2.3").unwrap());
}

/// Run jobs and draw until `done` holds.
fn until_settled(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        settle(h);
    }
    panic!("never saw {what}: {:?}", h.texts());
}

/// The KJV's header: the row under its tab, in the left Panel.
fn in_kjv_header(r: egui::Rect) -> bool {
    (80.0..100.0).contains(&r.top()) && r.right() < 930.0
}

/// A Collection, for the choosers that offer one.
fn night_reading(h: &mut Harness) {
    h.shell
        .run("collection.save", &json!({ "name": "Night reading" }));
    settle(h);
}

pub(super) fn run_split(h: &mut Harness, id: &str, params: Value) {
    let mut params = params;
    params["split"] = json!(true);
    h.shell.run(id, &params);
    settle(h);
    settle_all(h);
}

/// Run jobs until the search indexes are built and no search is running.
pub(super) fn settle_search(h: &mut Harness) {
    for _ in 0..5000 {
        h.shell.st.session.jobs().pump(16);
        h.frame();
        let s = &h.shell.st.session;
        if !s.search.is_building() && !s.bible_search_running() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the search indexes were never built");
}

/// The Search Tab alone in the window, its indexes built, having searched for "God", in
/// `mode`.
fn search(h: &mut Harness, mode: &str) -> TabId {
    // The launch's search cache is in its own folder.
    h.shell.st.session.refresh_search_indexes();
    settle_search(h);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = h.shell.session().workspace.focused().unwrap();
    let others: Vec<TabId> = (h.shell.session().workspace.tabs())
        .filter(|t| t.id != tab)
        .map(|t| t.id)
        .collect();
    for t in others {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    h.shell
        .run("search.mode", &json!({ "tab": tab.0, "value": mode }));
    h.shell
        .run("search.run", &json!({ "tab": tab.0, "text": "God" }));
    settle_search(h);
    settle_all(h);
    tab
}

/// The Notes Tab alone in the window: wide enough for its cards' … buttons.
fn notes_alone(h: &mut Harness) {
    h.shell.run("tool.notes", &json!({}));
    h.frames(2);
    let notes = h.shell.session().workspace.focused().unwrap();
    let others: Vec<TabId> = (h.shell.session().workspace.tabs())
        .filter(|t| t.id != notes)
        .map(|t| t.id)
        .collect();
    for t in others {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    settle(h);
    settle_all(h);
}

fn leftmost(h: &Harness, text: &str) -> egui::Pos2 {
    (h.shapes.iter())
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
        .center()
}

fn a_note(h: &mut Harness) {
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Gen 1:1", "body": "**Beginnings**: before all things." }),
    );
}

type Run = Overlay;

/// The menu bar's other menus and their submenus, and the toolbar's own menus.
fn menus_and_toolbar() -> Vec<Run> {
    vec![
        (
            "File menu",
            |h| h.open_menu("File"),
            |h| popup(h) && command_shown(h, "data.backups"),
        ),
        (
            "Edit menu",
            |h| h.open_menu("Edit"),
            |h| popup(h) && command_shown(h, "favorite.add"),
        ),
        (
            "View menu, Theme",
            |h| {
                h.open_menu("View");
                hover(h, "Theme");
            },
            |h| popup(h) && choice_shown(h, "view.theme"),
        ),
        (
            "View menu, Reading layout",
            |h| {
                h.open_menu("View");
                hover(h, "Reading layout");
            },
            |h| popup(h) && shows(h, "Large chapter numbers"),
        ),
        (
            "View menu, Notebooks in text",
            |h| {
                h.open_menu("View");
                hover(h, "Notebooks in text");
            },
            |h| popup(h) && shows(h, "My Notebook"),
        ),
        (
            "Go menu",
            |h| h.open_menu("Go"),
            |h| popup(h) && command_shown(h, "command_box.focus"),
        ),
        (
            "Tools menu",
            |h| h.open_menu("Tools"),
            |h| popup(h) && command_shown(h, "tool.timeline"),
        ),
        (
            "Window menu, Recently closed",
            |h| {
                let word_info = h.tab_id("Word Info");
                h.shell.run("tab.close", &json!({ "tab": word_info.0 }));
                h.frames(3);
                h.open_menu("Window");
                hover(h, "Recently closed");
            },
            |h| popup(h) && shows(h, "Word Info"),
        ),
        (
            "Window menu, Go to Panel",
            |h| {
                h.open_menu("Window");
                hover(h, "Go to Panel");
            },
            |h| popup(h) && command_shown(h, "panel.focus_9"),
        ),
        (
            "Toolbar menu, Position",
            |h| {
                let library = h.shell.st.toolbar.buttons[&Launcher::Library].center();
                right_click(h, library);
                hover(h, "Position");
            },
            |h| {
                popup(h)
                    && (obs_engine::ToolbarPosition::ALL.iter())
                        .all(|(_, _, label)| shows(h, label))
            },
        ),
        (
            "More launchers",
            |h| {
                h.screen = egui::vec2(1100.0, 600.0);
                h.frames(4);
                let more = h.shell.st.toolbar.overflow.expect("a » button");
                h.click(more.center());
            },
            |h| popup(h) && h.shell.st.toolbar.overflow.is_some(),
        ),
        (
            "Toolbar, auto-hidden, shown at its edge",
            |h| {
                h.shell.run("view.toolbar_auto_hide", &json!({}));
                h.events.push(Event::PointerMoved(egui::pos2(700.0, 1.0)));
                h.frames(4);
            },
            |h| h.shell.st.session.toolbar.auto_hide && h.shell.st.toolbar.bar.is_some(),
        ),
    ]
}

fn dialogs_and_popovers() -> Vec<Run> {
    vec![
        ("Selection toolbar", selection_toolbar, |h| {
            shows(h, "Search this") && shows(h, "Note")
        }),
        ("Note editor", note_editor, |h| shows(h, "Open in Notes")),
        (
            "Footnote popover",
            |h| {
                let bsb = open_bsb(h);
                let at = footnote_marker(h, bsb);
                h.events.push(Event::PointerMoved(at));
                until(h, "the Footnote's popover", |h| {
                    h.shell.st.footnotes.shown().is_some()
                });
                h.frames(10);
            },
            |h| h.shell.st.footnotes.shown().is_some(),
        ),
        (
            "Reference preview",
            |h| {
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
            },
            |h| shows(h, "Gen 1:3"),
        ),
        (
            "Tab switcher",
            |h| {
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
            },
            |h| h.shell.switcher.is_some(),
        ),
        (
            "Find bar",
            |h| {
                key(h, Key::F, Modifiers::COMMAND);
                type_text(h, "light");
                settle_all(h);
            },
            |h| h.shell.st.tabs[&h.tab_id("KJV")].find.is_some(),
        ),
        (
            "Move Note",
            |h| {
                let kjv = h.tab_id("KJV");
                h.shell.st.confirm_reanchor(
                    "n".into(),
                    "Genesis 1:1 (KJV)".into(),
                    kjv,
                    json!({}),
                    "Genesis 1:2 (KJV)".into(),
                );
                h.frames(3);
            },
            |h| h.shell.st.drag.confirm.is_some(),
        ),
        (
            "Delete Notebook",
            |h| {
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
                run_split(h, "tool.notes", json!({}));
                let p = h.at("Study");
                right_click(h, p);
                h.click_text("Delete…");
            },
            |h| h.shell.st.notes_ui.deleting.is_some(),
        ),
        (
            "Remove a Resource",
            |h| {
                h.shell.run("library.remove", &json!({ "id": "mhc" }));
                h.frames(3);
            },
            |h| h.shell.st.confirm.is_some(),
        ),
        (
            "Save layout as",
            |h| {
                h.shell.run("layout.save", &json!({}));
                h.frames(3);
                type_text(h, "Study desk");
            },
            |h| h.shell.st.prompt.is_some(),
        ),
        (
            "Import a Resource",
            |h| {
                h.shell.run("library.import", &json!({}));
                h.frames(3);
            },
            |h| h.shell.st.pick_import,
        ),
        (
            "Visual filters",
            |h| {
                h.shell.run("view.visual_filters", &json!({}));
                h.frames(3);
            },
            |h| h.shell.st.filters_ui.open,
        ),
        (
            "Language chooser",
            |h| {
                run_split(h, "library.get_resources", json!({}));
                h.click_text("All languages");
                // Off the button, so its tooltip is not drawn over the list.
                h.events
                    .push(Event::PointerMoved(egui::pos2(1395.0, 895.0)));
                h.frames(3);
            },
            |h| shows(h, "Search languages: name or code"),
        ),
        (
            "Page tone hint",
            |h| {
                // The hint offers Auto when the theme turns dark over a light Page tone the
                // user chose: the Look's own (Sepia) or else Paper. It stays open, back in the
                // Look's theme, until answered.
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
                // Past its fade-in.
                h.frames(15);
            },
            |h| shows_start(h, "Page tone is set to"),
        ),
        (
            "Update available",
            |h| {
                offer_updates(h, "v1.3.0");
                h.shell.run("help.check_for_updates", &json!({}));
                until_settled(h, "the update notice", |h| shows(h, "Update available"));
                h.frames(5);
            },
            |h| shows(h, "Update available") && shows(h, "Skip this version"),
        ),
        (
            "Update checks note",
            |h| {
                // The first launch that checks automatically says so, once.
                offer_updates(h, "v1.2.3");
                h.shell.arm_launch_update_check();
                let delay = obs_engine::updates::LAUNCH_CHECK_DELAY_SECS * 60.0;
                h.frames(delay as usize + 2);
                until_settled(h, "the first launch check's note", |h| {
                    h.shell.st.session.updates().first_run_note()
                });
                h.frames(5);
            },
            |h| shows(h, crate::shell::update_notice::FIRST_RUN_NOTE),
        ),
        (
            "About Open Bible",
            |h| {
                h.shell.run("help.about", &json!({}));
                h.frames(3);
            },
            |h| h.shell.st.window.is_some(),
        ),
    ]
}

/// The menus of Tab headers, rows and cards, and the choosers (drop-down lists) of Tabs and
/// windows.
fn menus_and_choosers() -> Vec<Run> {
    vec![
        (
            "Tab menu, Add to Favorites folder",
            |h| {
                let more = spot(h, "[ellipsis]", in_kjv_header);
                h.click(more);
                hover(h, "Add to Favorites folder");
            },
            |h| popup(h) && command_shown(h, "tab.close") && shows(h, "Add to Favorites folder"),
        ),
        (
            "Add Tab menu",
            |h| {
                let add = spot(h, "[plus]", |r| r.right() < 930.0);
                h.click(add);
            },
            |h| popup(h) && shows(h, "KJV — KJV (test)"),
        ),
        (
            "Tab right-click menu",
            |h| {
                let tab = h.tab_at("KJV");
                right_click(h, tab);
            },
            |h| popup(h) && shows(h, "Float in new window") && shows(h, "Link set B"),
        ),
        (
            "Link set menu",
            |h| {
                let chip = spot(h, "A", in_kjv_header);
                h.click(chip);
            },
            |h| popup(h) && shows(h, "Link set B") && !shows(h, "Float in new window"),
        ),
        (
            "Parallel resources chooser",
            |h| {
                let chooser = spot(h, "KJV", |r| in_kjv_header(r) && r.left() > 600.0);
                h.click(chooser);
            },
            |h| popup(h) && shows_start(h, "BSB — "),
        ),
        (
            "Filters menu",
            |h| {
                let filters = spot(h, "Filters", in_kjv_header);
                h.click(filters);
            },
            |h| popup(h) && shows(h, "Visual filters in this Tab"),
        ),
        (
            "Interlinear lines menu",
            |h| {
                let tab = h
                    .shell
                    .run(
                        "resource.open",
                        &json!({ "id": "tagnt-grace", "split": true }),
                    )
                    .unwrap()["tab"]
                    .as_u64()
                    .unwrap();
                h.shell
                    .run("tab.go", &json!({ "tab": tab, "text": "Lk 1:30" }));
                h.shell
                    .run("tab.interlinear", &json!({ "tab": tab, "on": true }));
                settle(h);
                settle_all(h);
                // "Int." in a narrow Tab.
                let label = h
                    .find("Interlinear")
                    .unwrap_or_else(|| h.find("Int.").unwrap());
                // Beside the Interlinear toggle.
                let lines = spot(h, "[chevron-down]", |r| {
                    (r.center().y - label.center().y).abs() < 10.0
                        && (r.center().x - label.center().x).abs() < 50.0
                });
                h.click(lines);
            },
            |h| popup(h) && shows(h, "Interlinear lines"),
        ),
        (
            "Text comparison, add a Bible",
            |h| {
                run_split(h, "tool.text_comparison", json!({ "passage": "Gen 1:1" }));
                h.click_text("+ Bible");
            },
            |h| popup(h) && (shows_start(h, "BSB — ") || shows(h, "Every Bible is shown")),
        ),
        (
            "Library row menu, Collections",
            |h| {
                night_reading(h);
                let row = spot(h, "KJV", |r| r.left() > 940.0 && r.top() > 200.0);
                right_click(h, row);
                hover(h, "Collections");
            },
            |h| popup(h) && shows(h, "Night reading") && shows(h, "Collections"),
        ),
        (
            "Notebook menu",
            |h| {
                a_note(h);
                run_split(h, "tool.notes", json!({}));
                let p = leftmost(h, "My Notebook");
                right_click(h, p);
            },
            |h| popup(h) && shows(h, "Rename…") && shows(h, "Delete…"),
        ),
        (
            "Note card menu, Move to Notebook",
            |h| {
                h.shell.run("notebook.create", &json!({ "name": "Study" }));
                a_note(h);
                notes_alone(h);
                h.click_text("…");
                hover(h, "Move to Notebook");
            },
            |h| popup(h) && shows(h, "Go to text") && shows(h, "Move to Notebook"),
        ),
        (
            "Highlight card menu, Highlight style",
            |h| {
                highlight_a_word(h);
                notes_alone(h);
                h.click_text("…");
                hover(h, "Highlight style");
            },
            |h| popup(h) && shows(h, "Highlight style") && shows(h, "Red underline"),
        ),
        (
            "Notes Export menu",
            |h| {
                a_note(h);
                run_split(h, "tool.notes", json!({}));
                h.click_text("Export…");
            },
            |h| popup(h) && shows(h, "Export as Markdown…"),
        ),
        (
            "Favorite menu, Move to folder",
            |h| {
                let kjv = h.tab_id("KJV");
                h.shell.run(
                    "favorite.add",
                    &json!({ "tab": kjv.0, "folder": "Sermons" }),
                );
                run_split(h, "tool.favorites", json!({}));
                let row = first(h, "Genesis 1:1 · KJV");
                right_click(h, row);
                hover(h, "Move to folder");
            },
            |h| popup(h) && shows(h, "No folder"),
        ),
        (
            "Search, Recent",
            |h| {
                search(h, "bible");
                h.click_text("Recent");
            },
            |h| popup(h) && shows(h, "God"),
        ),
        (
            "Search, scope presets",
            |h| {
                search(h, "bible");
                let scope = h.at("Scope");
                let presets = spot(h, "[chevron-down]", |r| {
                    (r.center().y - scope.y).abs() < 10.0 && r.left() > scope.x
                });
                h.click(presets);
            },
            |h| popup(h) && shows(h, "New Testament"),
        ),
        (
            "Search, add a Bible",
            |h| {
                search(h, "bible");
                h.click_text("+ Bible");
            },
            |h| popup(h) && shows_start(h, "BSB — "),
        ),
        (
            "Search, choose Resources",
            |h| {
                search(h, "library");
                h.click_text("Choose…");
            },
            |h| popup(h),
        ),
        (
            "Search, morphology chooser",
            |h| {
                search(h, "bible");
                h.click_text("Morphology…");
                let any = first(h, "Any");
                h.click(any);
            },
            |h| popup(h),
        ),
        (
            "Search, Notebook chooser",
            |h| {
                a_note(h);
                search(h, "notes");
                h.click_text("All Notebooks");
            },
            |h| popup(h) && shows(h, "My Notebook"),
        ),
        (
            "Search, chart chooser",
            |h| {
                let tab = search(h, "bible");
                h.shell
                    .run("search.view", &json!({ "tab": tab.0, "value": "chart" }));
                settle(h);
                settle_all(h);
                h.click_text("By chapter of…");
            },
            |h| popup(h) && shows(h, "Genesis"),
        ),
        (
            "Saved search menu",
            |h| {
                let tab = search(h, "bible");
                h.shell
                    .run("search.save", &json!({ "tab": tab.0, "name": "Creation" }));
                settle(h);
                if !shows(h, "SAVED SEARCHES") {
                    h.click_text("Saved & history");
                }
                let saved = h.at("Creation");
                right_click(h, saved);
            },
            |h| popup(h) && shows(h, "Run in a new split"),
        ),
        (
            "Highlight styles, palette chooser",
            |h| {
                h.shell.run("view.highlight_styles", &json!({}));
                h.frames(3);
                h.click_text("Standard");
            },
            |h| popup(h) && h.shell.st.styles_ui.open,
        ),
        (
            "Highlight styles, look chooser",
            |h| {
                h.shell.run("view.highlight_styles", &json!({}));
                h.frames(3);
                let look = first(h, "Background");
                h.click(look);
            },
            |h| popup(h) && shows(h, "Underline"),
        ),
        (
            "Highlight styles, colour presets",
            |h| {
                h.shell.run("view.highlight_styles", &json!({}));
                h.frames(3);
                let presets = spot(h, "[chevron-down]", |r| r.left() > 400.0);
                h.click(presets);
            },
            |h| popup(h) && h.shell.st.styles_ui.open,
        ),
        (
            "Highlight styles, where a deleted style's Highlights go",
            |h| {
                highlight_a_word(h);
                h.shell.run("view.highlight_styles", &json!({}));
                h.frames(10);
                let delete = first(h, "Delete…");
                h.click(delete);
                // The question makes the window taller: it moves to stay centred.
                h.frames(10);
                let question = (h.texts().into_iter())
                    .position(|t| t.contains("Move them to:"))
                    .map(|i| h.shapes[i].0)
                    .unwrap_or_else(|| panic!("no question: {:?}", h.texts()));
                let to = spot(h, "Green", |r| r.top() > question.bottom());
                h.click(to);
            },
            |h| popup(h) && h.texts().iter().any(|t| t.contains("Move them to:")),
        ),
        (
            "Visual filters, Also colour chooser",
            |h| {
                h.shell.run("view.visual_filters", &json!({}));
                h.frames(3);
                h.click_text("New filter…");
                h.click_text("Nothing else");
            },
            |h| popup(h) && h.shell.st.filters_ui.open,
        ),
        (
            "Visual filters, look chooser",
            |h| {
                h.shell.run("view.visual_filters", &json!({}));
                h.frames(3);
                h.click_text("New filter…");
                let look = h.at("Look");
                let chooser = spot(h, "Background", |r| (r.center().y - look.y).abs() < 10.0);
                h.click(chooser);
            },
            |h| popup(h) && shows(h, "Underline"),
        ),
        (
            "Fonts, font chooser",
            |h| {
                h.shell.run("view.fonts", &json!({}));
                h.frames(3);
                let latin = first(h, "Gentium Plus (bundled)");
                h.click(latin);
            },
            |h| popup(h) && h.shell.st.fonts_ui.open,
        ),
        (
            "Copy verses, preset chooser",
            |h| {
                h.shell
                    .run("tool.copy_verses", &json!({ "passages": "Gen 1:1-3" }));
                h.frames(3);
                // Its preview drawn, the window has its size and place.
                settle_all(h);
                h.frames(10);
                h.click_text("Custom");
            },
            |h| popup(h) && h.shell.st.copy_ui.open,
        ),
        (
            "Note editor, Notebook chooser",
            |h| {
                note_editor(h);
                let notebook = spot(h, "My Notebook", |r| r.left() < 900.0);
                h.click(notebook);
            },
            |h| popup(h) && shows(h, "Open in Notes"),
        ),
        (
            "Guide, Collection chooser",
            |h| {
                night_reading(h);
                run_split(h, "guide.passage_guide", json!({ "passage": "Jn 3:16" }));
                let chooser = first(h, "Every Resource");
                h.click(chooser);
            },
            |h| popup(h) && shows(h, "Night reading"),
        ),
        (
            "Reading plan, Bible chooser",
            |h| {
                run_split(h, "tool.reading_plans", json!({}));
                h.click_text("First Bible (in your Preferred order)");
            },
            |h| popup(h),
        ),
    ]
}

// ---------------------------------------------------------------------------------------
// Drawing.

/// A fresh launch with the gallery's fixtures in `look`.
pub(super) fn launch(name: &str, look: &Look) -> (Harness, Lab) {
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
            a_note(h);
            run_split(h, "tool.notes", json!({}));
        }
        Open::Tab(TabKind::Tool(Tool::Favorites)) => {
            let kjv = h.tab_id("KJV");
            h.shell.run("favorite.add", &json!({ "tab": kjv.0 }));
            run_split(h, "tool.favorites", json!({}));
        }
        Open::Tab(kind) => open_tab(h, lab, kind),
        Open::Run(open, _) => open(h),
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
                assert!(
                    is_open(&h, surface),
                    "{} is not on screen in {} at {scale_slug}: {:?}",
                    surface.title,
                    look.title,
                    h.texts()
                );
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

#[test]
fn gallery_menus_and_choosers() {
    gallery(Section::MenusAndChoosers);
}

#[test]
fn gallery_states() {
    gallery(Section::States);
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
         Every Tab kind, toolbar pane, menu, window, dialog, popover and chooser, and each \
         surface's empty, loading and error states, in each Look at 1× (2× linked). These are the goldens CI compares the app against; see \
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
    let mut seen = std::collections::HashSet::new();
    let shared: Vec<_> = names.iter().filter(|n| !seen.insert(*n)).collect();
    assert!(shared.is_empty(), "two surfaces share a name: {shared:?}");
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

/// Every Tab kind and every pane, menu and window of the accessibility audit is a surface.
#[test]
fn every_tab_kind_and_overlay_is_in_the_gallery() {
    let s = surfaces();
    for kind in every_tab_kind() {
        assert!(
            s.iter()
                .any(|s| matches!(&s.open, Open::Tab(k) if *k == kind)),
            "{kind:?} is not in the gallery"
        );
    }
    for (name, ..) in every_overlay() {
        assert!(
            s.iter().any(|s| s.title == name),
            "{name} is not in the gallery"
        );
    }
}

/// How `crates/ui-egui/src` opens a window, popover, menu or chooser.
const OPENERS: [&str; 8] = [
    "egui::Window::new(",
    "egui::Area::new(",
    "egui::Popup::menu(",
    "egui::Popup::context_menu(",
    ".context_menu(|",
    "components::menu(",
    "ui.menu_button(",
    "egui::ComboBox::from_id_salt(",
];

/// Each place in `crates/ui-egui/src` (outside the tests) that opens a window, popover, menu or
/// chooser, by file and [`OPENERS`]: one entry per place, in the file's order, naming the
/// gallery surfaces that draw it, or after `-` why none does.
const SITES: &[(&str, &str, &[&str])] = &[
    (
        "components.rs",
        "ui.menu_button(",
        &["- the Menu component itself: each use is listed under components::menu("],
    ),
    (
        "shell.rs",
        "egui::Window::new(",
        &["save-layout-as", "about-open-bible"],
    ),
    ("shell.rs", "egui::Area::new(", &["tab-reading-plans"]),
    ("shell.rs", "egui::Popup::menu(", &["link-set-menu"]),
    ("shell.rs", ".context_menu(|", &["tab-right-click-menu"]),
    (
        "shell.rs",
        "components::menu(",
        &[
            "file-menu edit-menu view-menu go-menu tools-menu window-menu-go-to-panel help-menu",
            "view-menu-theme",
            "view-menu-notebooks-in-text",
            "interlinear-lines-menu",
            "text-comparison-add-a-bible",
            "tab-menu-add-to-favorites-folder",
            "add-tab-menu",
        ],
    ),
    (
        "shell.rs",
        "ui.menu_button(",
        &[
            "window-menu-recently-closed",
            "window-menu-go-to-panel",
            "view-menu-reading-layout",
        ],
    ),
    (
        "shell.rs",
        "egui::ComboBox::from_id_salt(",
        &["parallel-resources-chooser"],
    ),
    (
        "shell/annotate.rs",
        "egui::Area::new(",
        &["selection-toolbar", "note-editor"],
    ),
    (
        "shell/annotate.rs",
        "egui::ComboBox::from_id_salt(",
        &["note-editor-notebook-chooser"],
    ),
    (
        "shell/collections_ui.rs",
        "egui::Window::new(",
        &["new-collection"],
    ),
    (
        "shell/collections_ui.rs",
        "components::menu(",
        &["library-row-menu-collections"],
    ),
    ("shell/command_box.rs", "egui::Area::new(", &["command-box"]),
    (
        "shell/context_menu.rs",
        "egui::Popup::context_menu(",
        &["right-click-menu"],
    ),
    (
        "shell/copy_window.rs",
        "egui::Window::new(",
        &["copy-verses"],
    ),
    (
        "shell/copy_window.rs",
        "egui::ComboBox::from_id_salt(",
        &["copy-verses-preset-chooser"],
    ),
    ("shell/drag_drop.rs", "egui::Window::new(", &["move-note"]),
    (
        "shell/favorites_tab.rs",
        ".context_menu(|",
        &["favorite-menu-move-to-folder"],
    ),
    (
        "shell/favorites_tab.rs",
        "components::menu(",
        &[
            "tab-menu-add-to-favorites-folder",
            "favorite-menu-move-to-folder",
        ],
    ),
    ("shell/fonts_window.rs", "egui::Window::new(", &["fonts"]),
    (
        "shell/fonts_window.rs",
        "egui::ComboBox::from_id_salt(",
        &[
            "fonts-font-chooser",
            "- a font family's styles: only for an installed family with several, and the \
             installed fonts differ by computer",
        ],
    ),
    (
        "shell/footnotes.rs",
        "egui::Area::new(",
        &["footnote-popover reference-preview"],
    ),
    (
        "shell/get_bibles_tab.rs",
        "egui::Popup::menu(",
        &["language-chooser"],
    ),
    (
        "shell/guide_tab.rs",
        "egui::ComboBox::from_id_salt(",
        &["guide-collection-chooser"],
    ),
    (
        "shell/highlight_styles_window.rs",
        "egui::Window::new(",
        &["highlight-styles"],
    ),
    (
        "shell/highlight_styles_window.rs",
        "egui::ComboBox::from_id_salt(",
        &[
            "highlight-styles-palette-chooser",
            "highlight-styles-look-chooser",
            "highlight-styles-where-a-deleted-style-s-highlights-go",
        ],
    ),
    (
        "shell/highlight_styles_window.rs",
        "components::menu(",
        &["highlight-styles-colour-presets"],
    ),
    (
        "shell/import_tab.rs",
        "egui::Window::new(",
        &["import-a-resource"],
    ),
    (
        "shell/keyboard_nav.rs",
        "egui::Area::new(",
        &["tab-switcher"],
    ),
    (
        "shell/library_tab.rs",
        "egui::Window::new(",
        &["remove-a-resource"],
    ),
    (
        "shell/library_tab.rs",
        ".context_menu(|",
        &["library-row-menu-collections"],
    ),
    (
        "shell/notes_tab.rs",
        "egui::Window::new(",
        &["delete-notebook"],
    ),
    (
        "shell/notes_tab.rs",
        ".context_menu(|",
        &[
            "notebook-menu",
            "- a card's right-click menu: its … button's menu (card_menu), drawn from the button",
        ],
    ),
    (
        "shell/notes_tab.rs",
        "components::menu(",
        &[
            "note-card-menu-move-to-notebook highlight-card-menu-highlight-style",
            "note-card-menu-move-to-notebook",
            "highlight-card-menu-highlight-style",
            "notes-export-menu",
        ],
    ),
    (
        "shell/page_tone.rs",
        "egui::Area::new(",
        &["page-tone-hint"],
    ),
    (
        "shell/reading_plans_tab.rs",
        "egui::ComboBox::from_id_salt(",
        &["reading-plan-bible-chooser"],
    ),
    (
        "shell/report_problem.rs",
        "egui::Window::new(",
        &["- Report a problem: it shows the operating system and the log (ADR 0019)"],
    ),
    (
        "shell/search_sidebar.rs",
        ".context_menu(|",
        &["saved-search-menu"],
    ),
    (
        "shell/search_tab.rs",
        "components::menu(",
        &[
            "search-recent",
            "search-scope-presets",
            "search-add-a-bible",
            "search-choose-resources",
        ],
    ),
    (
        "shell/search_tab.rs",
        "egui::ComboBox::from_id_salt(",
        &["search-morphology-chooser", "search-notebook-chooser"],
    ),
    (
        "shell/search_views.rs",
        "egui::ComboBox::from_id_salt(",
        &["search-chart-chooser"],
    ),
    (
        "shell/theme_editor_window.rs",
        "egui::Window::new(",
        &["theme-editor"],
    ),
    (
        "shell/toolbar.rs",
        "egui::Area::new(",
        &[
            "library-pane library-pane-searching",
            "tools-pane",
            "guides-pane",
            "layouts-pane",
        ],
    ),
    (
        "shell/toolbar_bar.rs",
        ".context_menu(|",
        &[
            "- the toolbar's menu (toolbar_menu) on its background: drawn as on a Launcher",
            "- the toolbar's menu (toolbar_menu) on its grip: drawn as on a Launcher",
            "toolbar-menu-position",
        ],
    ),
    (
        "shell/toolbar_bar.rs",
        "egui::Popup::menu(",
        &["more-launchers"],
    ),
    (
        "shell/toolbar_bar.rs",
        "egui::Area::new(",
        &["toolbar-auto-hidden-shown-at-its-edge"],
    ),
    (
        "shell/toolbar_bar.rs",
        "egui::Window::new(",
        &["customize-toolbar"],
    ),
    (
        "shell/toolbar_bar.rs",
        "components::menu(",
        &["toolbar-menu-position"],
    ),
    ("shell/tour_ui.rs", "egui::Area::new(", &["first-run-tour"]),
    (
        "shell/update_notice.rs",
        "egui::Area::new(",
        &["update-checks-note"],
    ),
    (
        "shell/update_notice.rs",
        "egui::Window::new(",
        &["update-available"],
    ),
    (
        "shell/visual_filters_window.rs",
        "egui::Window::new(",
        &["visual-filters"],
    ),
    (
        "shell/visual_filters_window.rs",
        "egui::ComboBox::from_id_salt(",
        &[
            "visual-filters-also-colour-chooser",
            "visual-filters-look-chooser",
        ],
    ),
    (
        "shell/visual_filters_window.rs",
        "ui.menu_button(",
        &[
            "filters-menu",
            "- a narrow Tab's ⋯ menu's Visual filters: the Filters menu's items (filters_items)",
        ],
    ),
];

/// The places in `crates/ui-egui/src` outside the tests that open a window, popover, menu or
/// chooser, by file (relative to `src`, with `/`) and opener, and how many.
fn opener_sites() -> std::collections::BTreeMap<(String, &'static str), usize> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.to_string_lossy().ends_with("_tests.rs")
            {
                out.push(path);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    let mut sites = std::collections::BTreeMap::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let file = path
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for opener in OPENERS {
            let n: usize = (text.lines())
                .filter(|l| !l.trim_start().starts_with("//"))
                .map(|l| l.matches(opener).count())
                .sum();
            if n > 0 {
                sites.insert((file.clone(), opener), n);
            }
        }
    }
    sites
}

/// Review (#222): a window, popover, menu or chooser the app opens is in the gallery, or the
/// gallery says why not. A new one fails here until a surface draws it.
#[test]
fn every_window_popover_menu_and_chooser_is_in_the_gallery() {
    let slugs: std::collections::HashSet<String> = surfaces().into_iter().map(|s| s.slug).collect();
    let listed: std::collections::BTreeMap<(String, &str), usize> = (SITES.iter())
        .map(|(file, opener, entries)| ((file.to_string(), *opener), entries.len()))
        .collect();
    assert_eq!(listed.len(), SITES.len(), "a file and opener listed twice");
    let found = opener_sites();
    let mut problems = Vec::new();
    for ((file, opener), n) in &found {
        let have = listed.get(&(file.clone(), *opener)).copied().unwrap_or(0);
        if have != *n {
            problems.push(format!(
                "src/{file} has {n} `{opener}` but SITES lists {have}: draw each new one in the \
                 gallery and name its surface there (or say why it is left out)"
            ));
        }
    }
    for key in listed.keys() {
        if !found.contains_key(key) {
            problems.push(format!(
                "SITES lists src/{} `{}`, no longer there",
                key.0, key.1
            ));
        }
    }
    for (file, opener, entries) in SITES {
        for entry in *entries {
            if entry.starts_with("- ") {
                continue;
            }
            for name in entry.split_whitespace() {
                if !slugs.contains(name) {
                    problems.push(format!("src/{file} `{opener}`: no surface {name}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
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
