//! The Workspace shell (#35): the engine's Panels and Tabs shown as an egui_tiles dock, each
//! Tab with a variant-B header strip — [link chip] [‹ ›] [box] [title] [⋯] — and the launcher
//! menus. Every menu item, + entry and header button runs a command through [`Session::run`];
//! what the commands change comes back as [`Effect`]s, which the shell applies to the dock.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Color32, RichText, Stroke, Ui, pos2, vec2};
use egui_tiles::{Container, Linear, LinearDir, Tile, TileId, Tiles, Tree, UiResponse};
use obs_engine::{
    Effect, HeaderBox, JobMode, LayoutNode, MENUS, MenuItem, PanelId, Placement, Session, Side,
    TabId, TabKind, Tool,
    interlinear::{InterlinearKind, InterlinearLine},
};
use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};
use serde_json::{Value, json};

use crate::compare_view::{ComparisonBible, ComparisonView, comparison_pane};
use crate::doc::ReadingDoc;
use crate::icons;
use crate::interlinear::InterlinearSource;
use crate::palette::Palette;
use crate::pane::{AtlasTexture, WordHit, reading_pane};
use crate::theme;
use crate::tokens::{inset, motion, radius, share, size, space, stroke, text};
use crate::view::ReadingView;
use crate::workers::SnippetCache;

mod annotate;
mod atlas_tab;
mod backups_tab;
mod collections_ui;
mod command_box;
mod contents_pane;
mod context_menu;
mod copy_window;
mod credits_tab;
mod drag_drop;
mod exegetical_guide;
mod export;
mod factbook_tab;
mod favorites_tab;
mod find_bar;
mod floating;
mod footnotes;
pub use floating::DesktopUnits;
mod fonts_window;
mod get_bibles_tab;
mod guide_parallels;
mod guide_tab;
mod guide_words;
mod highlight_styles_window;
mod home_tab;
mod import_tab;
mod keyboard_nav;
mod keys_tab;
mod library_tab;
mod missing_resource;
mod note_editor;
mod notes_tab;
mod page_tone;
mod panel_motion;
mod reading_plans_tab;
#[path = "references.rs"]
pub(crate) mod references;
pub(crate) mod report_problem;
mod search_sidebar;
mod search_tab;
mod search_views;
mod settings_tab;
mod starter_tab;
mod tile_handles;
mod timeline_tab;
mod toolbar;
mod toolbar_bar;
mod toolbar_look;
mod topic_guide_tab;
pub(crate) mod update_notice;
mod visual_filters_window;
mod word_info_tab;
mod word_sections_view;
mod word_study_tab;

/// How long the pointer rests on a Tagged word before Word Info follows it (#18 rule 16).
pub(crate) const HOVER_DWELL_SECS: f64 = 0.15;

/// The pointer resting on a Tagged word: where, since when, and whether Word Info has it.
#[derive(Debug, Clone, PartialEq)]
struct Dwell {
    tab: TabId,
    word: WordHit,
    since: f64,
    shown: bool,
}

/// Per-Tab UI state the engine does not need.
#[derive(Default)]
struct TabUi {
    view: Option<ReadingView>,
    /// The Visual filters' matches in the view's blocks (#211).
    filters: crate::filters::FilterPainter,
    /// The header box's text while it is being edited.
    edit: Option<String>,
    filter: String,
    error: Option<String>,
    /// A verse or Text range marked after a jump, and when the mark started.
    flash: Option<(crate::pane::Flash, f64)>,
    /// Library Tab: the language chips that are on, the selected row and the row whose
    /// handle is being dragged (resource ids).
    languages: Vec<String>,
    selected: Option<String>,
    dragging: Option<String>,
    /// Library Tab: the Collection chip that filters the list (#153).
    collection: Option<String>,
    /// About Tab: the Resource tags being edited (#153).
    tags_edit: Option<String>,
    /// A Text Comparison Tab's view.
    comparison: Option<ComparisonView>,
    /// A Search Tab's results view and its fields being edited.
    search: Option<crate::search_view::SearchView>,
    search_fields: search_tab::Fields,
    /// Failed selected Bibles are retried only on request (or after removal/reselection).
    comparison_errors: BTreeMap<String, String>,
    reference_at: Option<Passage>,
    expanded_references: std::collections::BTreeSet<Verse>,
    /// A Passage Guide section whose header is being dragged.
    guide_drag: Option<obs_engine::GuideSection>,
    /// The selection toolbar is up (from a selection's release until the next press).
    toolbar: bool,
    /// Where the selection was drawn last frame, in screen points.
    selection_rect: Option<egui::Rect>,
    /// What the right-click menu offers while it is open (#142).
    menu: Option<context_menu::MenuTargets>,
    /// Passage Guide sections showing all their rows (Key words, People & places).
    guide_expanded: std::collections::BTreeSet<obs_engine::GuideSection>,
    /// Word Study: Lexicon entries expanded (true) or collapsed by the user, by entry id, for
    /// the number in `lexicon_entries_for` (otherwise only the first is expanded).
    lexicon_open: BTreeMap<String, bool>,
    lexicon_entries_for: Option<obs_scripture::StrongsNumber>,
    /// A reopened Tab's scroll position (#214), put back once its pane has drawn at its real
    /// width.
    restore_scroll: Option<crate::view::TextAnchor>,
    /// Word Study (#56): the form filtering Occurrences (a [`obs_engine::Form::label`]) and
    /// whether Translated as shows every rendering, for `lexicon_entries_for`.
    occurrence_form: Option<String>,
    renderings_all: bool,
    /// Topic Guide (#148): the subtopics showing their Passages' text inline, for the topic
    /// in `topic_previews_for`.
    topic_previews: std::collections::BTreeSet<topic_guide_tab::SubtopicId>,
    topic_previews_for: Option<String>,
    /// Topic Guide: each subtopic row's width and height when last drawn, for the topic in
    /// `topic_previews_for`: a row out of view takes its place without being laid out.
    topic_rows: BTreeMap<topic_guide_tab::SubtopicId, (f32, f32)>,
    /// Where the header's Favorites ☆ was drawn last frame (#155).
    favorite_star: Option<egui::Rect>,
    /// History: "Clear History…" is asking to confirm.
    confirm_clear: bool,
    /// Where a Resource Tab was scrolled in each place it switched away from to a parallel
    /// resource (#151), oldest first; Back or Forward to that place scrolls there again.
    parked: Vec<ParkedScroll>,
    /// How wide the header's right-hand controls were last frame, so the header box leaves
    /// room for them.
    header_controls: f32,
    /// A Resource or Text Comparison Tab's Find bar, while it is open (Ctrl+F, #175).
    find: Option<crate::find::FindBar>,
    /// A Resource Tab's Contents pane: its filter, open lines and highlighted line (#176).
    contents: contents_pane::ContentsUi,
    /// An Atlas Tab's map: view, search and the place picked (#150).
    map: Option<atlas_tab::MapUi>,
    /// A Timeline Tab's view: where it looks and what it was fitted to (#185).
    timeline: Option<timeline_tab::TimelineUi>,
}

/// A Resource Tab's location when it switched to a parallel resource (#151), in that
/// Resource's own numbering or Keys, and where its view was scrolled there.
struct ParkedScroll {
    resource: String,
    at: ParkedAt,
    anchor: crate::view::TextAnchor,
}

#[derive(Debug, Clone, PartialEq)]
enum ParkedAt {
    Verse(Verse),
    Entry(Key),
}

impl ParkedAt {
    fn verse(&self) -> Option<Verse> {
        match self {
            ParkedAt::Verse(v) => Some(*v),
            ParkedAt::Entry(_) => None,
        }
    }
}

/// A Tab remembers at most this many parked scroll positions.
const PARKED_LIMIT: usize = 64;

/// Everything the dock's behaviour needs, apart from the tree itself.
pub(crate) struct State {
    pub(crate) session: Session,
    tabs: BTreeMap<TabId, TabUi>,
    docs: BTreeMap<String, Arc<ReadingDoc>>,
    atlas: AtlasTexture,
    references: references::References,
    toast: Option<(String, f64)>,
    window: Option<(String, String)>,
    /// The Layout-name prompt: the Layout being renamed (none: saving a new one) and the text.
    prompt: Option<(Option<String>, String)>,
    /// The Remove… confirmation: resource id, window title and message.
    confirm: Option<(String, String, String)>,
    pick_import: bool,
    /// The Get Bibles Tab's search and language filter.
    get_bibles_ui: get_bibles_tab::GetBiblesUi,
    picker: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// The Search Analysis's CSV save dialog, for its Tab (#146).
    csv_picker: Option<(TabId, std::sync::mpsc::Receiver<Option<std::path::PathBuf>>)>,
    /// When the Last session was last checked for saving.
    last_save: f64,
    /// This frame's Page-tone palette.
    pub(crate) palette: Palette,
    /// The toolbar's Command box (#139).
    pub(crate) command_box: command_box::CommandBoxUi,
    /// The toolbar's Launchers and their panes (#140).
    pub(crate) toolbar: toolbar::ToolbarUi,
    now: f64,
    /// Ctrl/Cmd is held: opening something splits off a new Panel.
    split_held: bool,
    /// egui_tiles Tabs containers ↔ engine Panels.
    pub(crate) panels: std::collections::HashMap<TileId, PanelId>,
    /// The Tagged word the pointer is resting on, for Word Info's dwell.
    dwell: Option<Dwell>,
    /// Inline-snippet layouts for Word Info's word and Lemma.
    snippets: SnippetCache,
    /// Highlights and Notes to draw, per Resource (#62).
    marks: annotate::MarkCache,
    /// The popup Note editor, when open.
    note_editor: Option<annotate::NoteEditor>,
    /// The Notes Tool's column, cards and the card being edited (#64).
    notes_ui: notes_tab::NotesUi,
    /// The Fonts window and the fonts loaded for View → Fonts (#138).
    fonts_ui: fonts_window::FontsUi,
    /// The Highlight styles window (View → Highlight styles, #181).
    styles_ui: highlight_styles_window::StylesUi,
    /// The Visual filters window (View → Visual filters, #211).
    pub(crate) filters_ui: visual_filters_window::FiltersUi,
    /// The Reading plans Tool's New plan form (#147).
    plans_ui: reading_plans_tab::PlansUi,
    /// The Copy verses window (#154).
    copy_ui: copy_window::CopyUi,
    /// The New Collection… / Edit… window (#153).
    collection_editor: Option<collections_ui::CollectionEditor>,
    /// A Notes export waiting for its save dialog (#154).
    export_ui: export::ExportUi,
    /// The system clipboard for rich text (Copy verses), when the app has one.
    rich_clipboard: Option<Arc<dyn crate::RichClipboard>>,
    /// Each Resource's Contents, built on a worker the first time its pane opens (#176).
    contents: contents_pane::ContentsCache,
    /// The Keyboard shortcuts Tab's key being recorded and conflict being resolved (#177).
    keys_ui: keys_tab::KeysUi,
    /// The Settings Tab's confirmations and search focus (#213).
    settings_ui: settings_tab::SettingsUi,
    /// The Back up & restore Tab's file dialog and restore preview (#217).
    backups_ui: backups_tab::BackupsUi,
    /// The Report a problem window and how folders are shown (#219).
    pub(crate) report_ui: report_problem::ReportProblemUi,
    /// The Last session has a save due that a later frame must make (#219): set when a
    /// frame's save was skipped, so a crash soon after still finds the Workspace saved.
    save_due: bool,
    /// The bench's feature scenarios (#183): one Tab's UI-thread work, timed each frame.
    pub(crate) timed_tab: TimedTab,
    /// The Footnote popover (#208).
    footnotes: footnotes::Footnotes,
    /// A reference, text or Note card being dragged across the Workspace (#216).
    pub(crate) drag: drag_drop::DragDropUi,
    /// A Tab whose button in its Panel's tab bar takes the keyboard when next drawn (#215).
    keyboard_to: Option<TabId>,
    /// Where F6 puts the keyboard in the menu bar and the Toolbar (#215).
    pub(crate) areas: keyboard_nav::AreaMarks,
    /// The hint offering Page tone Auto when the theme turns dark over a light tone (#240).
    page_tone_hint: page_tone::PageToneHint,
    /// The launch Update check's clock, the update notice and Restart now (#276).
    pub(crate) updates_ui: update_notice::UpdateUi,
}

/// A Tab whose drawing is timed each frame, for the bench's synchronous-work budget (#183,
/// #19: ≤ 2 ms per frame). Off (no Tab) outside the bench.
#[derive(Default)]
pub(crate) struct TimedTab {
    pub(crate) tab: Option<TabId>,
    /// Time the Tab's body took since last taken.
    pub(crate) took: Option<std::time::Duration>,
    /// Extra work added to the Tab's body, for the bench's seeded-slowdown check.
    pub(crate) slowdown: std::time::Duration,
}

/// The app window's content: menus, Command box and the docked Workspace.
pub struct Shell {
    pub(crate) st: State,
    pub(crate) tree: Tree<TabId>,
    shown: Vec<(PanelId, Vec<TabId>)>,
    /// The floating windows (#152), oldest first, each with its own dock.
    pub(crate) floating: Vec<floating::FloatingDock>,
    /// The screens (monitors) connected, the main window's first, as the app last told.
    screens: floating::Screens,
    /// The Tab last focused in each window (none: the main one), for the keys typed there.
    last_focused: BTreeMap<Option<obs_engine::WindowId>, TabId>,
    /// The Tab switcher, while Ctrl+Tab's Ctrl is held (#215).
    pub(crate) switcher: Option<keyboard_nav::TabSwitcher>,
    /// F6 (true) or Shift+F6 asked to move the keyboard to another area (#215).
    next_area: Option<bool>,
    /// A Tab the keyboard was sent to (#215), whose OS window is brought to the front at the
    /// end of the frame.
    os_focus_to: Option<TabId>,
    /// Text a command asked to put on the clipboard (Word Info's Copy), written at the end of
    /// the frame.
    copy_out: Option<String>,
    /// The operating system's reduce motion setting (#225).
    system_motion: Arc<dyn crate::SystemMotion>,
    /// The main dock's Panels settling into a new arrangement (#225).
    panel_motion: panel_motion::PanelMotion,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new(Session::new(JobMode::default_for_host()))
    }
}

impl Shell {
    pub fn new(session: Session) -> Self {
        Self {
            st: State {
                session,
                tabs: BTreeMap::new(),
                docs: BTreeMap::new(),
                atlas: AtlasTexture::default(),
                references: references::References::default(),
                toast: None,
                window: None,
                prompt: None,
                confirm: None,
                pick_import: false,
                get_bibles_ui: get_bibles_tab::GetBiblesUi::default(),
                picker: None,
                csv_picker: None,
                last_save: 0.0,
                palette: Palette::PAPER,
                command_box: command_box::CommandBoxUi::default(),
                toolbar: toolbar::ToolbarUi::default(),
                now: 0.0,
                split_held: false,
                panels: std::collections::HashMap::new(),
                dwell: None,
                snippets: SnippetCache::default(),
                marks: annotate::MarkCache::default(),
                note_editor: None,
                notes_ui: notes_tab::NotesUi::default(),
                fonts_ui: fonts_window::FontsUi::default(),
                styles_ui: highlight_styles_window::StylesUi::default(),
                filters_ui: visual_filters_window::FiltersUi::default(),
                plans_ui: reading_plans_tab::PlansUi::default(),
                copy_ui: copy_window::CopyUi::default(),
                collection_editor: None,
                export_ui: export::ExportUi::default(),
                rich_clipboard: None,
                contents: Default::default(),
                keys_ui: Default::default(),
                settings_ui: Default::default(),
                backups_ui: Default::default(),
                report_ui: Default::default(),
                save_due: false,
                timed_tab: TimedTab::default(),
                footnotes: Default::default(),
                drag: drag_drop::DragDropUi::default(),
                keyboard_to: None,
                areas: Default::default(),
                page_tone_hint: page_tone::PageToneHint::default(),
                updates_ui: update_notice::UpdateUi::default(),
            },
            tree: Tree::empty(egui::Id::unique("workspace")),
            shown: Vec::new(),
            floating: Vec::new(),
            screens: floating::Screens::default(),
            last_focused: BTreeMap::new(),
            switcher: None,
            next_area: None,
            os_focus_to: None,
            copy_out: None,
            system_motion: Arc::new(crate::NoSystemMotion),
            panel_motion: Default::default(),
        }
    }

    /// Where the operating system's reduce motion setting comes from (#225).
    pub fn set_system_motion(&mut self, motion: Arc<dyn crate::SystemMotion>) {
        self.system_motion = motion;
    }

    /// Whether motion is reduced: View → Reduce motion, or the operating system's setting.
    pub(crate) fn motion_reduced(&self) -> bool {
        self.st.session.appearance.reduce_motion || self.system_motion.reduce_motion()
    }

    /// Whether Resource Tab `tab` has drawn its text: the block at the top of its view is
    /// laid out (the scale bench's "first Bible text", #227).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn reading_text_shown(&self, tab: TabId) -> bool {
        (self.st.tabs.get(&tab))
            .and_then(|t| t.view.as_ref())
            .is_some_and(crate::view::ReadingView::top_block_laid)
    }

    pub fn session(&self) -> &Session {
        &self.st.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.st.session
    }

    /// Where installed fonts come from, for View → Fonts (#138). Without one, only the
    /// bundled fonts can be chosen.
    pub fn set_system_fonts(&mut self, fonts: Arc<dyn crate::SystemFonts>) {
        self.st.fonts_ui.set_source(fonts);
    }

    /// Check for updates automatically once this launch, a few seconds after the first frame
    /// (#276), when the setting is on. The desktop app arms it; tests and the bench do not.
    pub fn arm_launch_update_check(&mut self) {
        self.st.updates_ui.launch_check = update_notice::LaunchCheck::Armed;
    }

    /// The system clipboard, for copying rich text (Copy verses, #154).
    pub fn set_rich_clipboard(&mut self, clipboard: Arc<dyn crate::RichClipboard>) {
        self.st.rich_clipboard = Some(clipboard);
    }

    /// Run a command and apply what it changed. Failures become a toast.
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        let out = self.st.run(id, params);
        self.apply_effects();
        out
    }

    /// The default arrangement: the ★ Bible (first in Preferred order) in one Panel; the
    /// Library and Word Info in a Panel to its right.
    pub fn reset_layout(&mut self) {
        self.st.session.workspace.clear();
        self.apply_effects();
        self.tree = Tree::empty(egui::Id::unique("workspace"));
        self.st.panels.clear();
        self.floating.clear();
        let first = self
            .st
            .session
            .library
            .preferred(ResourceType::Bible)
            .map(|e| e.manifest.resource_id.clone());
        if let Some(id) = first {
            // The first Bible starts in Link set A, so Bibles opened from it follow along.
            let bible = self.run("resource.open", &json!({ "id": id }));
            if let Some(t) = bible.and_then(|v| v["tab"].as_u64()) {
                self.run("tab.link", &json!({ "tab": t, "set": "A" }));
            }
        }
        let library = self.run("library.open", &json!({}));
        self.run("tool.word_info", &json!({}));
        // The Library is the useful Tab there until Word Info has content (#50).
        if let Some(t) = library.and_then(|v| v["tab"].as_u64()) {
            self.st.session.workspace.focus(TabId(t));
            self.apply_effects();
        }
        if let Some(Tile::Container(Container::Linear(l))) =
            self.tree.root().and_then(|r| self.tree.tiles.get_mut(r))
            && let [a, b] = l.children[..]
        {
            l.shares.set_share(a, 2.0);
            l.shares.set_share(b, 1.0);
        }
        // Focus the Bible.
        let bible = self
            .st
            .session
            .workspace
            .tabs()
            .find(|t| t.kind.is_reading())
            .map(|t| t.id);
        if let Some(t) = bible {
            self.st.session.workspace.focus(t);
            self.apply_effects();
        }
    }

    /// Apply queued engine effects to the dock.
    pub fn apply_effects(&mut self) {
        for e in self.st.session.workspace.take_effects() {
            // A reopened Tab not drawn yet (#214) that is sent elsewhere first stays there.
            if let Some(t) = e.moved_view().and_then(|tab| self.st.tabs.get_mut(&tab)) {
                t.restore_scroll = None;
            }
            match e {
                Effect::Opened { tab, placement } => {
                    self.st.opened(tab);
                    self.insert_tab(tab, placement);
                }
                Effect::Focus(tab) => self.make_active(tab),
                Effect::Closed(tab) => {
                    self.remove_pane(tab);
                    self.st.tabs.remove(&tab);
                    self.st.footnotes.tab_closed(tab);
                    let live: Vec<TabId> = self.st.session.workspace.tabs().map(|t| t.id).collect();
                    self.st.references.retain_tabs(&live);
                }
                Effect::RestoreScroll { tab, at } => self.st.restore_scroll(tab, &at),
                Effect::KeyboardToTab(tab) => {
                    self.make_active(tab);
                    self.st.keyboard_to(tab);
                    self.os_focus_to = Some(tab);
                }
                Effect::NextArea { forward } => self.next_area = Some(forward),
                Effect::Float {
                    tab,
                    window,
                    panel,
                    placement,
                } => self.float_pane(tab, window, panel, placement),
                Effect::Dock { tab, panel } => {
                    self.remove_pane(tab);
                    insert_pane(
                        &mut self.tree,
                        &mut self.st.panels,
                        tab,
                        Placement::right_of(panel, None),
                    );
                }
                Effect::DockWindow(window) => self.dock_floating(window),
                Effect::Navigate { tab, verse } => {
                    let now = self.st.now;
                    let eased = !self.motion_reduced();
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                    {
                        let from = view.shown_position();
                        if view.jump_to(verse) {
                            if eased {
                                view.glide_from(from, motion::JUMP, motion::ease_out);
                            }
                            t.flash = Some((crate::pane::Flash::Verse(verse), now));
                        } else {
                            self.st
                                .toast(format!("No section or Verse for {verse} in this Resource"));
                        }
                    }
                }
                Effect::NavigateEntry { tab, key } => {
                    if let Some(view) = self.st.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) {
                        view.jump_to_entry(&key);
                    }
                }
                Effect::Follow { tab, verse, flash } => {
                    let now = self.st.now;
                    let eased = !self.motion_reduced();
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                    {
                        let from = view.shown_position();
                        let followed = view.follow(verse);
                        // A follow that waits for its layout lands when it arrives, at once.
                        if followed && eased && !view.is_following() {
                            view.glide_from(from, motion::JUMP, motion::ease_out);
                        }
                        if followed && flash {
                            t.flash = Some((crate::pane::Flash::Verse(verse), now));
                        }
                    }
                }
                Effect::NavigateText {
                    tab,
                    unit,
                    range,
                    key,
                } => {
                    let now = self.st.now;
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                    {
                        let index = view.doc.units.iter().position(|u| u.id == unit);
                        let (start, end) = (range.start as usize, range.end as usize);
                        if let Some(i) = index
                            && view.jump_to_text(i, start)
                        {
                            let flash = crate::pane::Flash::Text {
                                unit: i,
                                range: start..end,
                            };
                            t.flash = Some((flash, now));
                        } else {
                            // The text is not shown (a note): its Key, at least.
                            let shown = match &key {
                                Some(Key::Passage(p)) => view.jump_to(p.start()),
                                Some(k) => view.jump_to_entry(k),
                                None => false,
                            };
                            if !shown {
                                self.st
                                    .toast(format!("“{unit}” is not shown in this Resource"));
                            }
                        }
                    }
                }
                Effect::ShowUnit { tab, unit } => self.st.show_unit(tab, &unit),
                Effect::ResetLayout => self.reset_layout(),
                Effect::Rebuild => self.rebuild(),
                Effect::PromptLayoutName { rename } => {
                    let text = rename.clone().unwrap_or_default();
                    self.st.prompt = Some((rename, text));
                }
                Effect::Message(m) => self.st.toast(m),
                Effect::Show { title, body } => self.st.window = Some((title, body)),
                Effect::ConfirmRemove {
                    resource_id,
                    title,
                    message,
                } => self.st.confirm = Some((resource_id, title, message)),
                Effect::PickImport => self.st.pick_import = true,
                Effect::ShowFonts => self.st.fonts_ui.open = true,
                Effect::OpenFolder(path) => self.st.open_folder(&path),
                Effect::ReportProblem => self.st.show_report_problem(),
                Effect::SaveDiagnosticsZip => self.st.show_diagnostics_zip(),
                Effect::Restart(exe) => self.restart_now(&exe),
                Effect::ShowHighlightStyles => self.st.styles_ui.open = true,
                Effect::ShowVisualFilters => self.st.filters_ui.open = true,
                Effect::FocusCommandBox => {
                    self.st.toolbar.close();
                    self.st.command_box.focus = true;
                }
                Effect::ListCommands => {
                    self.st.toolbar.close();
                    self.st.command_box.list_commands();
                }
                Effect::ConfirmSetting { tab, setting } => self.st.confirm_setting(tab, setting),
                Effect::CopyText(text) => {
                    self.copy_out = Some(text);
                    self.st.toast("Copied");
                }
                Effect::OpenLauncher(which) => self.st.toolbar.toggle(which),
                Effect::Find { tab, text } => self.st.open_find(tab, text),
                Effect::FindStep { tab, forward } => self.st.step_find(tab, forward),
                Effect::CustomizeToolbar => {
                    self.st.toolbar.close();
                    self.st.toolbar.customizing = true;
                }
                Effect::ShowCopyVerses { passages, bibles } => {
                    self.st.open_copy_verses(passages, bibles);
                }
                Effect::Removed(id) => {
                    self.st.docs.remove(&id);
                    self.st.marks.forget(&id);
                    self.st.copy_ui.removed(&id);
                }
                Effect::Replaced(id) => {
                    self.st.docs.remove(&id);
                    self.st.marks.forget(&id);
                    self.st.copy_ui.replaced(&id);
                    // Its open Tabs (and every Text Comparison) load the new text, where they
                    // were.
                    let reload: Vec<TabId> = self
                        .st
                        .session
                        .workspace
                        .tabs()
                        .filter(|t| match &t.kind {
                            TabKind::Resource(r) | TabKind::About(r) => *r == id,
                            TabKind::Tool(Tool::TextComparison) => true,
                            _ => false,
                        })
                        .map(|t| t.id)
                        .collect();
                    for tab in reload {
                        if self.st.tabs.contains_key(&tab) {
                            self.st.opened(tab);
                        }
                    }
                    self.st.note_editor_replaced(&id);
                    self.st.lexicon_replaced(&id);
                }
                Effect::Switched(tab) => self.st.switched(tab),
                Effect::NavigateStandard { tab, verse } => {
                    self.st.session.workspace.navigate_standard(tab, verse);
                }
                Effect::NavigateOpened { tab, verse, key } => {
                    let ws = &mut self.st.session.workspace;
                    if let Some(v) = verse {
                        ws.navigate(tab, v);
                    }
                    if let Some(key) = key {
                        ws.navigate_entry(tab, key);
                    }
                }
                Effect::RevealText { tab, unit, byte } => {
                    let now = self.st.now;
                    let shown = self
                        .st
                        .tabs
                        .get_mut(&tab)
                        .and_then(|t| {
                            let view = t.view.as_mut()?;
                            let u = view.doc.units.iter().position(|x| x.id == unit)?;
                            if !view.jump_to_pos(u, byte) {
                                return None;
                            }
                            if let Some(v) = view.doc.verse_containing(u, byte) {
                                t.flash = Some((crate::pane::Flash::Verse(v), now));
                            }
                            Some(())
                        })
                        .is_some();
                    if !shown {
                        self.st
                            .toast("That text is no longer in this Resource".to_string());
                    }
                }
            }
        }
        // The views have moved: what each shows is where the next history entry is pushed from.
        let tabs: Vec<TabId> = self.st.tabs.keys().copied().collect();
        for tab in tabs {
            self.st.report_view_unit(tab);
        }
        self.record_layout();
        // New Tabs are placed now: History records where they and jumped Tabs are (#155).
        self.st.session.record_history();
    }

    fn insert_tab(&mut self, tab: TabId, placement: Placement) {
        // Into the window that holds the Panel it goes into or beside (#152).
        let near = match placement {
            Placement::Panel(p) => Some(p),
            Placement::NewPanel { beside, .. } => beside,
        };
        let dock = near.and_then(|p| {
            self.floating
                .iter()
                .position(|d| container_of(&d.tree, &d.panels, p).is_some())
        });
        match dock {
            Some(i) => {
                let d = &mut self.floating[i];
                insert_pane(&mut d.tree, &mut d.panels, tab, placement);
            }
            None => insert_pane(&mut self.tree, &mut self.st.panels, tab, placement),
        }
    }

    /// Tell the engine what the docks now show (after drags, docks and × closes), in the
    /// main window and every floating window.
    fn record_layout(&mut self) {
        // Right after a drop, egui_tiles can leave a pane outside any Tabs container until its
        // next simplify; do it now so a moved Tab is never mistaken for a closed one.
        let options = egui_tiles::Behavior::simplification_options(&self.st);
        self.tree.simplify(&options);
        let ws = &mut self.st.session.workspace;
        let mut shown = shown_panels(&self.tree, &mut self.st.panels, ws);
        for d in &mut self.floating {
            d.tree.simplify(&options);
            shown.extend(shown_panels(&d.tree, &mut d.panels, ws));
        }
        // A floating window left with no Tabs closes.
        self.floating.retain(|d| !d.tree.is_empty());
        shown.sort();
        if shown != self.shown {
            self.st.session.workspace.sync_panels(shown.clone());
            self.shown = shown;
            let live: Vec<TabId> = self.st.session.workspace.tabs().map(|t| t.id).collect();
            self.st.tabs.retain(|t, _| live.contains(t));
            self.st.footnotes.retain_tabs(&live);
            self.st.references.retain_tabs(&live);
        }
        let arrangement = self
            .tree
            .root()
            .and_then(|r| dock_node(&self.tree, &self.st.panels, r));
        self.st.session.workspace.arrangement = arrangement;
        self.st.session.workspace.floating = self
            .floating
            .iter()
            .map(|d| obs_engine::FloatingWindow {
                id: d.id,
                arrangement: d.tree.root().and_then(|r| dock_node(&d.tree, &d.panels, r)),
                placement: d.placement,
            })
            .collect();
    }

    /// Rebuild the dock and every Tab's view from the engine's restored Workspace.
    fn rebuild(&mut self) {
        self.tree = Tree::empty(egui::Id::unique("workspace"));
        self.st.panels.clear();
        self.st.tabs.clear();
        self.st.footnotes.tabs.clear();
        // Clicks and jumps made in the Workspace just replaced wait for Tabs whose ids the
        // restored ones may reuse: they go with it (#259).
        self.st.references.retain_tabs(&[]);
        self.shown.clear();
        let ids: Vec<TabId> = self.st.session.workspace.tabs().map(|t| t.id).collect();
        for t in ids {
            self.st.opened(t);
        }
        let ws = &mut self.st.session.workspace;
        if let Some(node) = ws.arrangement.clone() {
            self.tree.root = build_node(&mut self.tree, &mut self.st.panels, ws, &node);
        }
        // The floating windows come back where they were (#152).
        self.floating = ws
            .floating
            .clone()
            .into_iter()
            .filter_map(|w| {
                let mut d = floating::FloatingDock::new(w.id, w.placement);
                let node = w.arrangement?;
                d.tree.root = build_node(&mut d.tree, &mut d.panels, ws, &node);
                d.tree.root.map(|_| d)
            })
            .collect();
        if let Some(f) = self.st.session.workspace.focused() {
            self.make_active(f);
        }
    }

    /// Show a Tab in its Panel, in whichever window it is.
    fn make_active(&mut self, tab: TabId) {
        let is = |_: TileId, t: &Tile<TabId>| matches!(t, Tile::Pane(p) if *p == tab);
        if !self.tree.make_active(is) {
            for d in &mut self.floating {
                if d.tree.make_active(is) {
                    break;
                }
            }
        }
    }

    /// Take a Tab's pane out of whichever window shows it.
    fn remove_pane(&mut self, tab: TabId) {
        if let Some(id) = self.tree.tiles.find_pane(&tab) {
            self.tree.remove_recursively(id);
        }
        for d in &mut self.floating {
            if let Some(id) = d.tree.tiles.find_pane(&tab) {
                d.tree.remove_recursively(id);
            }
        }
    }

    /// Follow the Theme choice (Follow system tracks the OS live) and pick this frame's
    /// chrome and page palettes. Nothing is laid out or rasterized again: colour roles are
    /// resolved when quads are drawn.
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let want = match self.st.session.appearance.theme {
            obs_engine::Theme::Light => egui::ThemePreference::Light,
            obs_engine::Theme::Dark => egui::ThemePreference::Dark,
            obs_engine::Theme::System => egui::ThemePreference::System,
        };
        if ctx.options(|o| o.theme_preference) != want {
            ctx.set_theme(want);
        }
        theme::set_for(ctx);
        crate::key_labels::set_for(ctx);
        // Interface size is the only whole-window zoom (#137). egui's own Ctrl+Plus/Minus/0
        // zoom is off: those keys are Text size. A stale zoom is put back at once.
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        let zoom = self.st.session.appearance.interface_size.zoom();
        if ctx.zoom_factor() != zoom {
            ctx.set_zoom_factor(zoom);
        }
        crate::motion::set(ctx, self.motion_reduced());
        let dark = theme::c().dark;
        let tone = self.st.session.appearance.page_tone.resolve(dark);
        self.st.palette = Palette::for_tone(tone);
        self.st.follow_page_tone_hint(dark);
    }

    /// Show the Resource `id` given at startup (a file on the command line) on top of what
    /// [`Self::restore_at_launch`] opened, Home included: its most recently active Tab is
    /// focused, else it opens.
    pub fn show_startup_resource(&mut self, id: &str) {
        let kind = TabKind::Resource(id.to_string());
        match self.st.session.workspace.most_recent(|t| t.kind == kind) {
            Some(tab) => {
                self.st.session.workspace.focus(tab);
                self.apply_effects();
            }
            None => {
                self.run("resource.open", &json!({ "id": id }));
            }
        }
    }

    /// Save the Last session now (on quit), or at most once a second while running. A
    /// skipped save asks for the frame that makes it, so the Workspace is saved within a
    /// second of a change even when the app then idles (and survives a crash, #219).
    pub fn save_session(&mut self, now: bool) {
        let since = self.st.now - self.st.last_save;
        if !now && since < 1.0 {
            self.st.save_due = true;
            return;
        }
        self.st.save_due = false;
        self.st.last_save = self.st.now;
        if let Err(e) = self.st.session.save_last_session() {
            self.st.toast(format!("Could not save the session: {e}"));
        }
    }

    /// Restart now (#276): save any Note being written and the Last session first, as closing
    /// does, so the new app reads them; then start it and close this window. A Note that
    /// cannot be saved stops the restart and shows why, as it stops closing.
    fn restart_now(&mut self, exe: &std::path::Path) {
        self.st.save_note();
        self.st.save_card_edit(true);
        if let Some(id) = self.st.notes_ui.editing.as_ref().map(|e| e.id.clone()) {
            self.st.run("notes.open", &json!({ "id": id }));
        }
        if self.st.note_editor.is_some() || self.st.notes_ui.editing.is_some() {
            self.st
                .toast("Save or discard the Note being written, then click Restart now again.");
            return;
        }
        self.save_session(true);
        self.st.restart_into(exe);
    }

    /// Restore the Last session if there is one, else open the default layout. Returns
    /// whether there was one.
    pub fn restore_or_default(&mut self) -> bool {
        if self.st.session.restore_last_session() {
            self.apply_effects();
            true
        } else {
            self.reset_layout();
            false
        }
    }

    /// The Workspace at launch: the Last session (else the default layout), with Home on top
    /// on the first run (no Last session) and at every launch when "Open Home at launch" is
    /// on (#178).
    pub fn restore_at_launch(&mut self) {
        let restored = self.restore_or_default();
        if !restored || self.st.session.home_opens_at_launch() {
            self.run("home.open", &json!({}));
        }
    }

    /// How soon to draw again without input: job events are applied only when a frame
    /// polls them, so running work keeps frames coming while the app is idle. Background
    /// Search index builds (#58) need only a slow poll.
    fn background_repaint(&self) -> Option<std::time::Duration> {
        // Bible searches and their snippet pages (#59) are applied by a frame too.
        if self.st.session.imports.is_running()
            || self.st.session.starter.download().is_some()
            || self.st.session.starter.is_checking()
            || self.st.picker.is_some()
            || self.st.export_waiting()
            || self.st.data_dialog_waiting()
            || self.st.zip_dialog_waiting()
            || self.st.session.guides.is_busy()
            || self.st.session.bible_search_running()
            || self.st.session.word_study.is_busy()
            || self.st.session.reanchoring.is_busy()
            || self.st.session.factbooks.is_busy()
            || self.st.session.topic_guides.is_busy()
            || self.st.session.updates().is_downloading()
        {
            Some(std::time::Duration::from_millis(16))
        } else if self.st.session.search.is_building()
            || self.st.folder_opening()
            || self.st.session.updates().is_busy()
        {
            Some(std::time::Duration::from_millis(250))
        } else {
            None
        }
    }

    /// Draw the whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Back at the front, the user may have changed the system's reduce motion setting.
        if ui.input(|i| (i.events.iter()).any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            self.system_motion.refresh();
        }
        self.apply_appearance(ui.ctx());
        crate::motion::instant_scrolling(ui.ctx());
        self.st.apply_fonts(ui.ctx());
        self.st.references.refresh(&self.st.session.library);
        self.st.now = ui.input(|i| i.time);
        self.st.split_held = ui.input(|i| i.modifiers.command);
        self.note_main_window(ui.ctx());
        scroll_focus_into_view(ui.ctx());
        // Poll, never wait: parsing, Resource writes, Search-index hooks and Guide sections
        // live on workers. Without worker threads (wasm), Guide sections and the People &
        // places directory step here, a job a frame.
        if !self.st.session.jobs().is_threaded()
            && (self.st.session.guides.is_busy()
                || self.st.session.word_study.is_busy()
                || self.st.session.reanchoring.is_busy()
                || self.st.session.factbooks.is_busy()
                || self.st.session.topic_guides.is_busy())
        {
            self.st.session.jobs().pump(1);
        }
        let _other_jobs = self.st.session.poll_imports();
        // A Starter library Tab restored with the session needs the released manifest too;
        // here, not at restore, so it comes through the app's transport.
        self.st.session.load_starter_manifest_for_open_tab();
        self.st.poll_picker();
        self.st.poll_export();
        self.st.poll_data_dialog();
        self.st.poll_csv_picker();
        self.st.poll_zip_dialog();
        self.st.poll_folder_opening();
        self.st.launch_update_check(ui.ctx());
        self.apply_effects();
        if let Some(after) = self.background_repaint() {
            ui.ctx().request_repaint_after(after);
        }
        self.st.keys_ui.begin_frame();
        self.shortcuts(ui, None);
        // F6's marks are drawn again this frame (the Toolbar not at all while Auto-hidden).
        self.st.areas = Default::default();
        self.st.toolbar.begin_frame();
        let menus = egui::Panel::top("menus")
            .frame(
                egui::Frame::new()
                    .fill(theme::c().surface)
                    .inner_margin(inset::BAND),
            )
            .show(ui, |ui| self.menu_bar(ui));
        self.st.areas.menus = menus.inner.map(|id| (id, menus.response.rect));
        self.st.toolbar.menus_bottom = menus.response.rect.bottom();
        // The toolbar at the Bottom, Left or Right, or Auto-hidden (#141); then its open pane.
        self.toolbar_outside_menus(ui);
        self.launcher_pane(ui);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().shell)
                    .inner_margin(inset::POPOVER),
            )
            .show(ui, |ui| {
                if self.tree.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(
                                "Nothing is open. Use File → Library, or drop a .obr file here.",
                            )
                            .color(theme::c().muted),
                        );
                    });
                    self.st.workspace_drag(ui, &[]);
                } else {
                    self.main_drag_out(ui);
                    let before = tile_handles::widgets_so_far(ui);
                    self.tree.ui(&mut self.st, ui);
                    let reduce = crate::motion::reduced(ui.ctx());
                    let now = self.st.now;
                    let settling = (self.panel_motion).after_tree(
                        ui,
                        &self.tree,
                        &self.st.panels,
                        now,
                        reduce,
                    );
                    if settling {
                        ui.ctx().request_repaint();
                    }
                    tile_handles::after_tree(ui, &mut self.tree, before);
                    let panels = drag_drop::drop_panels(
                        &self.tree,
                        &self.st.panels,
                        &self.st.session.workspace,
                    );
                    self.st.workspace_drag(ui, &panels);
                }
            });
        self.apply_effects();
        self.show_floating_windows(ui.ctx());
        // A Note being written on a Tab in a floating OS window is drawn in that window.
        let note_here =
            (self.st.note_editor.as_ref()).is_none_or(|ed| self.keys_window(ed.tab).is_none());
        // So is the Move Note box for a Note dropped on a Tab there.
        let reanchor_here =
            (self.st.drag.confirm.as_ref()).is_none_or(|r| self.keys_window(r.tab).is_none());
        self.st.overlays(ui, note_here, reanchor_here);
        self.customize_window(ui.ctx());
        focus_ring(ui.ctx());
        // A Notes card being edited saves as the user types, and at once once its Tab is not
        // drawn (#64): checked here, since the Notes Tab's own code only runs while drawn.
        self.st.notes_after_frame(ui.ctx());
        self.apply_effects();
        self.st.close_after_restart(ui.ctx());
        self.focus_os_window(ui.ctx());
        if let Some(text) = self.copy_out.take() {
            ui.ctx().copy_text(text);
        }
        crate::motion::ease_popups(ui.ctx());
        let closing = ui.input(|i| i.viewport().close_requested());
        if closing {
            // A Note being written is saved, as if focus had left it. One that cannot be saved
            // (in the Note editor or on a Notes card) keeps the window open, showing why, until
            // it is saved or discarded.
            self.st.save_note();
            self.st.save_card_edit(true);
            if let Some(id) = self.st.notes_ui.editing.as_ref().map(|e| e.id.clone()) {
                // Show the card that did not save, with its message and Discard changes.
                self.st.run("notes.open", &json!({ "id": id }));
            }
            if self.st.note_editor.is_some() || self.st.notes_ui.editing.is_some() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        self.save_session(closing);
        if self.st.save_due {
            let left = (1.0 - (self.st.now - self.st.last_save)).max(0.0);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }

    /// Offer Report a problem for a new Crash report in the Log folder (at launch, #219).
    pub fn offer_crash_report(&mut self) {
        self.st.offer_crash_report();
    }

    /// The keys typed in `window` (none: the main one), on the Tab focused there
    /// ([`Self::focus_in`]). The window gaining the keyboard, or a key run in it, makes that
    /// Tab the focused one.
    fn shortcuts(&mut self, ui: &mut Ui, window: Option<obs_engine::WindowId>) {
        if let Some(f) = self.st.session.workspace.focused() {
            self.last_focused.insert(self.keys_window(f), f);
        }
        let focused = self.focus_in(window);
        let gained =
            ui.input(|i| (i.events.iter()).any(|e| matches!(e, egui::Event::WindowFocused(true))));
        if gained {
            self.focus_here(focused);
        }
        // A key being recorded in the Keyboard shortcuts Tab (#177) runs nothing in its window.
        if self.st.keys_ui.is_recording_in(ui) {
            return;
        }
        self.tab_switcher_keys(ui, window);
        // Keys a text field uses itself (Ctrl+Z, Esc …) are left to it while it has focus.
        let typing = ui.ctx().text_edit_focused();
        // A Footnote popover shown in this window has first refusal on Esc (#208): a command
        // bound to Esc (Unpin Word Info, say) waits for the next one.
        let footnote_shown =
            (self.st.footnotes).on_screen(ui.ctx(), |t| self.keys_window(t) == window);
        let all = bindings(&self.st.session);
        let session = &self.st.session;
        let bound: Vec<(&obs_engine::CommandSpec, egui::KeyboardShortcut)> = session
            .commands()
            .iter()
            .filter_map(|spec| Some((spec, session.shortcut(spec.id)?)))
            .filter(|(_, key)| !(typing && annotate::yields_to_text_fields(key)))
            .filter_map(|(spec, key)| Some((spec, parse_shortcut(key)?)))
            .filter(|(_, sc)| {
                !(footnote_shown && sc.logical_key == egui::Key::Escape && sc.modifiers.is_none())
            })
            .flat_map(|(spec, sc)| pressed_as(sc).map(move |sc| (spec, sc)))
            .collect();
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        let mut switch: Vec<(bool, egui::Modifiers)> = Vec::new();
        for (spec, sc) in bound {
            let needs_tab = spec.params.iter().any(|p| p.name == "tab" && p.required);
            let other_required = spec.params.iter().any(|p| p.required && p.name != "tab");
            if other_required || (needs_tab && focused.is_none()) {
                continue;
            }
            // A disabled command leaves its key to whoever else wants it (Esc, say).
            if !self.st.session.is_enabled(spec.id) {
                continue;
            }
            if ui.input_mut(|i| consume_key(i, sc.modifiers, sc.logical_key, extra_held(sc, &all)))
            {
                // Ctrl+Tab and Ctrl+Shift+Tab open the Tab switcher (#215).
                if let Some(i) = keyboard_nav::SWITCHER_COMMANDS
                    .iter()
                    .position(|c| *c == spec.id)
                {
                    switch.push((i == 0, sc.modifiers));
                    continue;
                }
                let params = match (needs_tab, focused) {
                    (true, Some(t)) => json!({ "tab": t.0 }),
                    _ => json!({}),
                };
                run.push((spec.id, params));
            }
        }
        if !run.is_empty() || !switch.is_empty() {
            self.focus_here(focused);
        }
        for (id, p) in run {
            self.run(id, &p);
        }
        if let Some(tab) = focused {
            for (forward, modifiers) in switch {
                self.tab_switcher_step(window, tab, forward, modifiers);
            }
        }
        if self.st.annotation_keys(ui, focused) {
            self.focus_here(focused);
        }
        self.apply_effects();
        self.cycle_area(ui.ctx(), window);
        self.tab_switcher_ui(ui.ctx(), window);
    }

    /// The Tab focused in the window whose keys are being read becomes the focused one.
    fn focus_here(&mut self, tab: Option<TabId>) {
        let ws = &mut self.st.session.workspace;
        if let Some(t) = tab
            && ws.focused() != Some(t)
        {
            ws.user_focused(t);
        }
    }

    /// The menu bar: the app's name, the menus (File · Edit · View · Go · Tools · Window · Help,
    /// #226), the Launchers (#140) when the toolbar is at the Top (#141), and the Command box
    /// on the right. Returns the first menu's id, where F6 puts the keyboard (#215).
    fn menu_bar(&mut self, ui: &mut Ui) -> Option<egui::Id> {
        let mut first = None;
        ui.horizontal(|ui| {
            ui.label(crate::components::title_text("Open Bible"));
            ui.add_space(space::L);
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.scope(|ui| {
                let w = &mut ui.visuals_mut().widgets;
                w.inactive.bg_stroke = Stroke::NONE;
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.hovered.bg_stroke = Stroke::NONE;
                for menu in MENUS {
                    let r = crate::components::menu(ui, menu.title, |ui| {
                        for item in menu.items {
                            self.menu_item(ui, *item);
                        }
                    });
                    first = first.or(Some(r.response.id));
                }
            });
            let toolbar = &self.st.session.toolbar;
            if toolbar.position == obs_engine::ToolbarPosition::Top && !toolbar.auto_hide {
                ui.add_space(space::M);
                let room = ui.available_width() - self.st.toolbar.reserve;
                self.launcher_strip(ui, room, false);
            }
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.command_box(ui);
                // Today's reading (#147), left of the Command box, and Home (#178).
                ui.add_space(space::M);
                if self.st.todays_reading_button(ui) {
                    self.apply_effects();
                }
                if self.st.home_button(ui) {
                    self.apply_effects();
                }
                ui.min_rect().width()
            });
            // What the Launchers leave room for next frame: the right side and gaps.
            self.st.toolbar.reserve = right.inner + space::M + 2.0 * ui.spacing().item_spacing.x;
        });
        first
    }

    fn menu_item(&mut self, ui: &mut Ui, item: MenuItem) {
        match item {
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Command(id) => {
                let Some(spec) = self.st.session.commands().get(id).copied() else {
                    return;
                };
                // A toggle shows a check while it is on; a command with an icon shows it.
                let checked = match id {
                    "view.red_letter" => Some(self.st.session.text.red_letter),
                    "view.highlights" => Some(self.st.session.appearance.show_highlights),
                    "view.toolbar_auto_hide" => Some(self.st.session.toolbar.auto_hide),
                    "view.footnote_markers" => Some(
                        self.st.session.is_enabled(id)
                            && (self.st.session.workspace.focused())
                                .and_then(|t| self.st.session.workspace.tab(t))
                                .is_some_and(|t| t.footnote_markers),
                    ),
                    _ => None,
                };
                let label = match id {
                    "library.get_resources" => {
                        match self.st.session.resource_updates().updates.len() {
                            0 => spec.label.to_string(),
                            1 => format!("{} (1 update)", spec.label),
                            n => format!("{} ({n} updates)", spec.label),
                        }
                    }
                    _ => spec.label.to_string(),
                };
                let icon = match checked {
                    Some(true) => Some(icons::Icon::Check),
                    Some(false) => None,
                    None => icons::for_command(id),
                };
                // Without an icon (an unchecked toggle), the icon's room is left blank so the
                // menu's labels line up.
                let mut b = match icon {
                    Some(icon) => crate::components::button(icons::with_icon(ui, icon, label)),
                    None => crate::components::button((icons::blank(), label)),
                };
                if let Some(sc) = self.st.session.shortcut(id) {
                    b = b.shortcut_text(crate::key_labels::label(sc));
                }
                // A command that needs a Tab runs on the focused one (#226).
                let needs_tab = spec.params.iter().any(|p| p.name == "tab" && p.required);
                let focused = self.st.session.workspace.focused();
                let has_tab = !needs_tab || focused.is_some();
                let enabled = self.st.session.is_enabled(id) && has_tab;
                let r = ui.add_enabled(enabled, b);
                if let Some(on) = checked {
                    icons::announce_toggled(&r, on);
                }
                let r = if enabled {
                    r
                } else if has_tab {
                    r.on_disabled_hover_text(not_yet(id))
                } else {
                    r.on_disabled_hover_text("Open a Tab first")
                };
                if r.clicked() {
                    let params = match focused {
                        Some(t) if needs_tab => json!({ "tab": t.0 }),
                        _ => json!({}),
                    };
                    self.run(id, &params);
                    ui.close();
                }
            }
            MenuItem::Choice { title, command } => {
                let (options, current) = obs_engine::choices(&self.st.session, command);
                let mut picked = None;
                crate::components::menu(ui, (icons::blank(), title), |ui| {
                    for (id, label) in options {
                        if ui.selectable_label(id == current, label).clicked() {
                            picked = Some(id);
                            ui.close();
                        }
                    }
                });
                if let Some(id) = picked {
                    self.run(command, &json!({ "value": id }));
                    ui.close();
                }
            }
            MenuItem::Notebooks => {
                // Each Notebook's Show in text (#17 rule 18); the Notes Tool (#64) shows the
                // same toggles beside its Notebooks.
                let notebooks = if self.st.session.user.is_some() {
                    self.st.session.notebooks().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut toggled = None;
                let enabled = self.st.session.is_enabled("notebook.show_in_text");
                ui.add_enabled_ui(enabled, |ui| {
                    crate::components::menu(ui, (icons::blank(), "Notebooks in text"), |ui| {
                        for n in &notebooks {
                            let mut on = n.show_in_text;
                            if crate::components::toggle(ui, &mut on, &n.name).clicked() {
                                toggled = Some((n.id.clone(), on));
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text("Notebooks are kept in the User database");
                });
                if let Some((id, show)) = toggled {
                    self.run("notebook.show_in_text", &json!({ "id": id, "show": show }));
                }
            }
            MenuItem::RecentlyClosed => {
                // Recently closed (#214), newest first: each reopens in its old Panel.
                let closed = self.st.session.recently_closed_labels();
                let mut reopen = None;
                ui.add_enabled_ui(!closed.is_empty(), |ui| {
                    ui.menu_button("Recently closed", |ui| {
                        for (id, label) in &closed {
                            if ui.button(label).clicked() {
                                reopen = Some(*id);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text("No Tab has been closed yet");
                });
                if let Some(id) = reopen {
                    self.run("tab.reopen_closed", &json!({ "closed": id }));
                    ui.close();
                }
            }
            MenuItem::Submenu { title, items } => {
                ui.menu_button((icons::blank(), title), |ui| {
                    for item in items {
                        self.menu_item(ui, *item);
                    }
                });
            }
            MenuItem::ReadingLayout => self.reading_layout_menu(ui),
            // Only the Library and Layouts Launchers' panes list these.
            MenuItem::Favorites | MenuItem::Layouts | MenuItem::Resources => {}
        }
    }
}

impl Shell {
    /// View → Reading layout (#209): the focused Resource Tab's own choices, each run as
    /// `tab.reading_layout`; what it has not chosen follows the defaults in Settings.
    fn reading_layout_menu(&mut self, ui: &mut Ui) {
        let workspace = &self.st.session.workspace;
        let tab = workspace.focused().filter(|t| {
            matches!(
                workspace.tab(*t).map(|t| &t.kind),
                Some(TabKind::Resource(_))
            )
        });
        let mut run: Option<Value> = None;
        ui.add_enabled_ui(tab.is_some(), |ui| {
            ui.menu_button((icons::blank(), "Reading layout"), |ui| {
                let Some(tab) = tab else {
                    return;
                };
                let shown = self.st.session.reading_layout(tab);
                for (flow, id, label) in obs_engine::TEXT_FLOWS {
                    if ui.radio(shown.flow == flow, label).clicked() {
                        run = Some(json!({ "flow": id }));
                    }
                }
                ui.separator();
                let toggles = [
                    (
                        "large_chapter_numbers",
                        "Large chapter numbers",
                        shown.large_chapter_numbers,
                    ),
                    (
                        "section_headings",
                        "Section headings",
                        shown.section_headings,
                    ),
                    ("justified", "Justified text", shown.justified),
                ];
                for (param, label, mut on) in toggles {
                    if ui.checkbox(&mut on, label).clicked() {
                        run = Some(json!({ param: on }));
                    }
                }
                ui.separator();
                ui.label(
                    RichText::new("Reading column width")
                        .small()
                        .color(theme::c().muted),
                );
                for (column, id, label) in obs_engine::COLUMN_WIDTHS {
                    if ui.radio(shown.column == column, label).clicked() {
                        run = Some(json!({ "column": id }));
                    }
                }
                ui.separator();
                let own = (self.st.session.workspace.tab(tab))
                    .is_some_and(|t| !t.reading_layout.is_default());
                if ui
                    .add_enabled(own, egui::Button::new("Use defaults"))
                    .on_hover_text("Follow the Reading layout defaults in Settings")
                    .clicked()
                {
                    run = Some(json!({ "reset": true }));
                }
            })
            .response
            .on_disabled_hover_text("Focus a Resource Tab to choose its Reading layout");
        });
        if let (Some(mut params), Some(tab)) = (run, tab) {
            params["tab"] = json!(tab.0);
            self.run("tab.reading_layout", &params);
        }
    }
}

/// Which ticket a not-yet-built menu item arrives with.
fn not_yet(id: &str) -> &'static str {
    match id {
        "library.import" => "Import a local USFM file or folder",
        "layout.save" => "Saved Layouts arrive with #37",
        "word_info.word_study" => "Rest the pointer on a word with a Strong's number first",
        _ => "Not available yet",
    }
}

fn replace_child(c: &mut Container, old: TileId, new: TileId) {
    let children = match c {
        Container::Tabs(t) => &mut t.children,
        Container::Linear(l) => &mut l.children,
        Container::Grid(_) => {
            c.add_child(new);
            return;
        }
    };
    if let Some(slot) = children.iter_mut().find(|c| **c == old) {
        *slot = new;
    }
}

/// Which engine Panel each Tabs container of one window's dock shows.
pub(crate) type PanelTiles = std::collections::HashMap<TileId, PanelId>;

fn container_of(tree: &Tree<TabId>, panels: &PanelTiles, panel: PanelId) -> Option<TileId> {
    panels
        .iter()
        .find(|(t, p)| **p == panel && tree.tiles.get(**t).is_some())
        .map(|(t, _)| *t)
}

/// Put a Tab's pane into a window's dock: into Panel `p` there, or as a new Panel split off
/// on its side of `beside` (or of the whole dock).
fn insert_pane(tree: &mut Tree<TabId>, panels: &mut PanelTiles, tab: TabId, placement: Placement) {
    let pane = tree.tiles.insert_pane(tab);
    if let Placement::Panel(p) = placement
        && let Some(c) = container_of(tree, panels, p)
        && let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(c)
    {
        tabs.add_child(pane);
        tabs.set_active(pane);
        return;
    }
    let (panel, beside, side, levels) = match placement {
        Placement::Panel(p) => (p, None, Side::Right, 0),
        Placement::NewPanel {
            panel,
            beside,
            side,
            levels,
        } => (panel, beside, side, levels),
    };
    let container = tree.tiles.insert_tab_tile(vec![pane]);
    panels.insert(container, panel);
    let anchor = beside.and_then(|b| container_of(tree, panels, b));
    attach_beside(tree, container, anchor, side, levels);
}

/// Put `tile` on `side` of `anchor` (none: of the whole dock), or of the row or column
/// `levels` out from it: into the row or column holding that when it runs the same way,
/// else in a new one with it.
fn attach_beside(
    tree: &mut Tree<TabId>,
    tile: TileId,
    anchor: Option<TileId>,
    side: Side,
    levels: u8,
) {
    let Some(root) = tree.root() else {
        tree.root = Some(tile);
        return;
    };
    let mut anchor = anchor.unwrap_or(root);
    for _ in 0..levels {
        match tree.tiles.parent_of(anchor) {
            Some(p) => anchor = p,
            None => break,
        }
    }
    let dir = if side.in_row() {
        LinearDir::Horizontal
    } else {
        LinearDir::Vertical
    };
    let parent = tree.tiles.parent_of(anchor);
    if let Some(parent) = parent
        && let Some(Tile::Container(Container::Linear(l))) = tree.tiles.get_mut(parent)
        && l.dir == dir
    {
        let at = l
            .children
            .iter()
            .position(|c| *c == anchor)
            .map_or(l.children.len(), |i| if side.before() { i } else { i + 1 });
        l.children.insert(at, tile);
        return;
    }
    let children = if side.before() {
        vec![tile, anchor]
    } else {
        vec![anchor, tile]
    };
    let split = tree
        .tiles
        .insert_new(Tile::Container(Container::Linear(Linear::new(
            dir, children,
        ))));
    match parent {
        None => tree.root = Some(split),
        Some(parent) => {
            if let Some(Tile::Container(c)) = tree.tiles.get_mut(parent) {
                replace_child(c, anchor, split);
            }
        }
    }
}

/// Each Panel a window's dock shows, with its Tabs in order. Tabs containers new since the
/// last call (made by a drag) get a fresh Panel id; containers gone are forgotten.
fn shown_panels(
    tree: &Tree<TabId>,
    panels: &mut PanelTiles,
    ws: &mut obs_engine::Workspace,
) -> Vec<(PanelId, Vec<TabId>)> {
    let mut shown = Vec::new();
    for id in tree.tiles.tile_ids() {
        let Some(Tile::Container(Container::Tabs(t))) = tree.tiles.get(id) else {
            continue;
        };
        let tabs: Vec<TabId> = t
            .children
            .iter()
            .filter_map(|c| match tree.tiles.get(*c) {
                Some(Tile::Pane(p)) => Some(*p),
                _ => None,
            })
            .collect();
        let panel = *panels.entry(id).or_insert_with(|| ws.new_panel_id());
        shown.push((panel, tabs));
    }
    panels.retain(|t, _| tree.tiles.get(*t).is_some());
    shown
}

/// A window's dock below `tile` as a saved arrangement.
fn dock_node(tree: &Tree<TabId>, panels: &PanelTiles, tile: TileId) -> Option<LayoutNode> {
    match tree.tiles.get(tile)? {
        Tile::Pane(t) => Some(LayoutNode::Panel {
            panel: 0,
            tabs: vec![t.0],
            active: Some(t.0),
        }),
        Tile::Container(Container::Tabs(t)) => {
            let tabs: Vec<u64> = t
                .children
                .iter()
                .filter_map(|c| match tree.tiles.get(*c) {
                    Some(Tile::Pane(p)) => Some(p.0),
                    _ => None,
                })
                .collect();
            let active = t.active.and_then(|a| match tree.tiles.get(a) {
                Some(Tile::Pane(p)) => Some(p.0),
                _ => None,
            });
            let panel = panels.get(&tile).map_or(0, |p| p.0);
            (!tabs.is_empty()).then_some(LayoutNode::Panel {
                panel,
                tabs,
                active,
            })
        }
        Tile::Container(Container::Linear(l)) => {
            let children: Vec<(LayoutNode, f32)> = l
                .children
                .iter()
                .filter_map(|c| Some((dock_node(tree, panels, *c)?, l.shares[*c])))
                .collect();
            if children.is_empty() {
                return None;
            }
            Some(match l.dir {
                LinearDir::Horizontal => LayoutNode::Row { children },
                LinearDir::Vertical => LayoutNode::Column { children },
            })
        }
        Tile::Container(Container::Grid(g)) => {
            let children: Vec<(LayoutNode, f32)> = g
                .children()
                .filter_map(|c| Some((dock_node(tree, panels, *c)?, 1.0)))
                .collect();
            (!children.is_empty()).then_some(LayoutNode::Row { children })
        }
    }
}

/// Insert a saved arrangement into a window's dock; returns its root tile. Tabs the
/// Workspace does not have are left out.
fn build_node(
    tree: &mut Tree<TabId>,
    panels: &mut PanelTiles,
    ws: &mut obs_engine::Workspace,
    node: &LayoutNode,
) -> Option<TileId> {
    match node {
        LayoutNode::Panel {
            panel,
            tabs,
            active,
        } => {
            let live: Vec<TabId> = tabs
                .iter()
                .map(|t| TabId(*t))
                .filter(|t| ws.tab(*t).is_some())
                .collect();
            if live.is_empty() {
                return None;
            }
            let panes: Vec<TileId> = live.iter().map(|t| tree.tiles.insert_pane(*t)).collect();
            let container = tree.tiles.insert_tab_tile(panes.clone());
            if let Some(Tile::Container(Container::Tabs(t))) = tree.tiles.get_mut(container)
                && let Some(i) = active.and_then(|a| live.iter().position(|t| t.0 == a))
            {
                t.set_active(panes[i]);
            }
            let panel = if *panel == 0 {
                ws.new_panel_id()
            } else {
                PanelId(*panel)
            };
            panels.insert(container, panel);
            Some(container)
        }
        LayoutNode::Row { children } | LayoutNode::Column { children } => {
            let built: Vec<(TileId, f32)> = children
                .iter()
                .filter_map(|(c, share)| Some((build_node(tree, panels, ws, c)?, *share)))
                .collect();
            if built.len() == 1 {
                return built.first().map(|(t, _)| *t);
            }
            if built.is_empty() {
                return None;
            }
            let dir = if matches!(node, LayoutNode::Row { .. }) {
                LinearDir::Horizontal
            } else {
                LinearDir::Vertical
            };
            let mut linear = Linear::new(dir, built.iter().map(|(t, _)| *t).collect());
            for (t, share) in &built {
                linear.shares.set_share(*t, *share);
            }
            Some(
                tree.tiles
                    .insert_new(Tile::Container(Container::Linear(linear))),
            )
        }
    }
}

/// Every key binding in the app as the user has them (#177): the commands' and the Selection
/// keys.
pub(crate) fn bindings(session: &obs_engine::Session) -> Vec<egui::KeyboardShortcut> {
    (session.commands().iter())
        .filter_map(|spec| session.shortcut(spec.id).and_then(parse_shortcut))
        .flat_map(pressed_as)
        .chain(annotate::shortcuts(session))
        .collect()
}

/// `text (key)` with the key `id` has now (#177) as the platform writes it (#262), or `text`
/// alone when it has none.
pub(crate) fn with_key(session: &obs_engine::Session, text: &str, id: &str) -> String {
    match session.shortcut(id) {
        Some(key) => format!("{text} ({})", crate::key_labels::label(key)),
        None => text.to_string(),
    }
}

/// The key `id` has now as the platform writes it (#262); empty when it has none.
pub(crate) fn key_of(session: &obs_engine::Session, id: &str) -> String {
    session
        .shortcut(id)
        .map(crate::key_labels::label)
        .unwrap_or_default()
}

/// A hint that Undo brings back what was just done: `Edit → Undo brings it back (⌘Z)` naming
/// Undo's place in the menu bar, or `Undo (⌘Z) brings it back` while Undo is in no menu, so
/// the hint never names a menu that is not there (#262). `brings` is the rest of the sentence.
pub(crate) fn undo_hint(session: &obs_engine::Session, brings: &str) -> String {
    match session.menu_path("edit.undo") {
        Some(path) => with_key(session, &format!("{path} {brings}"), "edit.undo"),
        None => format!("{} {brings}", with_key(session, "Undo", "edit.undo")),
    }
}

/// The key reports that press `sc`: itself, and for `+` also `=`. On US, UK and AZERTY
/// keyboards `+` is Shift+=, so Ctrl+Plus is pressed as Ctrl+= too, as in browsers (#137).
/// The numpad's + and − report Plus and Minus.
///
/// Ctrl+Tab is also the Control key with Tab on macOS, where Cmd+Tab switches apps and never
/// reaches the app (#215).
pub(crate) fn pressed_as(
    sc: egui::KeyboardShortcut,
) -> impl Iterator<Item = egui::KeyboardShortcut> {
    let also = (sc.logical_key == egui::Key::Plus)
        .then(|| egui::KeyboardShortcut::new(sc.modifiers, egui::Key::Equals));
    let control = (sc.logical_key == egui::Key::Tab && sc.modifiers.command).then(|| {
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: false,
            mac_cmd: false,
            ..sc.modifiers
        };
        egui::KeyboardShortcut::new(modifiers, egui::Key::Tab)
    });
    std::iter::once(sc).chain(also).chain(control)
}

/// Which of Shift and Alt may be held on top of `sc`'s own modifiers and still press it.
///
/// Usually both, as egui matches: some keys need them on some layouts (Ctrl+Plus is
/// Ctrl+Shift+= on a US keyboard). None when another binding in `all` is the same key with the
/// same Ctrl (or Cmd) and other Shift or Alt, so the two are told apart exactly: Ctrl+Shift+Z
/// is never Ctrl+Z, even when there is nothing to redo. A binding with other Ctrl is told
/// apart by Ctrl itself (Ctrl+1 a Panel, Alt+1 a Highlight, #215). Never an extra Alt on
/// Ctrl+digit: Ctrl+Alt is AltGr on Windows, which types { [ ] } on digits on some layouts.
pub(crate) fn extra_held(
    sc: egui::KeyboardShortcut,
    all: &[egui::KeyboardShortcut],
) -> egui::Modifiers {
    let same_ctrl = |m: egui::Modifiers| {
        (m.ctrl, m.command, m.mac_cmd)
            == (
                sc.modifiers.ctrl,
                sc.modifiers.command,
                sc.modifiers.mac_cmd,
            )
    };
    if (all.iter()).any(|o| {
        o.logical_key == sc.logical_key && o.modifiers != sc.modifiers && same_ctrl(o.modifiers)
    }) {
        return egui::Modifiers::NONE;
    }
    use egui::Key::{Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9};
    let digit = matches!(
        sc.logical_key,
        Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9
    );
    egui::Modifiers {
        shift: true,
        alt: !(digit && (sc.modifiers.command || sc.modifiers.ctrl)),
        ..egui::Modifiers::NONE
    }
}

/// Consume presses of `key` with `modifiers`, plus any of the Shift and Alt in `extra`.
/// Shift or Alt held but in neither does not match.
pub(crate) fn consume_key(
    i: &mut egui::InputState,
    modifiers: egui::Modifiers,
    key: egui::Key,
    extra: egui::Modifiers,
) -> bool {
    let before = i.events.len();
    i.events.retain(|e| {
        let egui::Event::Key {
            key: k,
            modifiers: m,
            pressed: true,
            ..
        } = e
        else {
            return true;
        };
        let held = egui::Modifiers {
            shift: m.shift && !(extra.shift && !modifiers.shift),
            alt: m.alt && !(extra.alt && !modifiers.alt),
            ..*m
        };
        !(*k == key && held.matches_exact(modifiers))
    });
    i.events.len() != before
}

/// `Ctrl+Shift+L` → an egui shortcut. Ctrl means Cmd on macOS.
pub fn parse_shortcut(s: &str) -> Option<egui::KeyboardShortcut> {
    let mut mods = egui::Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part {
            "Ctrl" => mods |= egui::Modifiers::COMMAND,
            "Shift" => mods |= egui::Modifiers::SHIFT,
            "Alt" => mods |= egui::Modifiers::ALT,
            k => key = egui::Key::from_name(k),
        }
    }
    key.map(|k| egui::KeyboardShortcut::new(mods, k))
}

impl State {
    fn toast(&mut self, m: impl Into<String>) {
        self.toast = Some((m.into(), self.now));
    }

    /// Every way a command runs (a button, a menu, the Command box, a key) comes here, so a
    /// restore or its undo (#217) always saves the Notes being written first and forgets the
    /// Notes Tab's drafts after.
    fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        let pinned = (id == "command_box.run")
            .then(|| self.command_box_replacing_user_data(params))
            .flatten();
        let params = pinned.as_ref().unwrap_or(params);
        let replaces_user_data = backups_tab::replaces_user_data(id) || pinned.is_some();
        if replaces_user_data && !self.settle_note_editors() {
            return None;
        }
        let out = match self.session.run(id, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(e.to_string());
                None
            }
        };
        if replaces_user_data && out.is_some() {
            self.forget_notes_drafts();
        }
        out
    }

    /// When `command_box.run` with `params` runs a restore or its undo (`>Undo restore`,
    /// #226), so it is guarded as from a menu or a key: its params with that command found
    /// once. Saving the Notes first can change the suggestions (an Undo appears), so the
    /// command guarded is the one run.
    fn command_box_replacing_user_data(&mut self, params: &Value) -> Option<Value> {
        if let Some(command) = params["command"].as_str() {
            return backups_tab::replaces_user_data(command).then(|| params.clone());
        }
        let text = params["text"].as_str().unwrap_or_default();
        let choice = (params["choice"].as_u64())
            .and_then(|c| usize::try_from(c).ok())
            .unwrap_or(0);
        let (kept, action) = self.session.command_box_choice(text, choice)?;
        match action {
            obs_engine::CommandBoxAction::Command(command)
                if backups_tab::replaces_user_data(command) =>
            {
                Some(json!({
                    "text": kept,
                    "command": command,
                    "new_tab": params["new_tab"].as_bool().unwrap_or(false),
                }))
            }
            _ => None,
        }
    }

    /// Report where Resource Tab `tab`'s view, just drawn, is scrolled to the Workspace,
    /// which Recently closed takes it from if the user closes the Tab (#214). A reopened Tab
    /// not drawn yet reports nothing: it still carries the position it closed with.
    fn report_scroll(&mut self, tab: TabId) {
        let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) else {
            return;
        };
        let at = view.text_anchor().and_then(|a| {
            Some(obs_engine::ScrollPosition {
                unit: view.doc.units.get(a.unit)?.id.clone(),
                byte: a.byte,
                into: a.into,
            })
        });
        let ws = &mut self.session.workspace;
        if ws.tab(tab).is_some_and(|t| t.scroll != at) {
            ws.view_scrolled(tab, at);
        }
    }

    /// A reopened Tab (#214), loaded at its Current location, scrolls back to where its view
    /// was when it closed, once its pane has drawn.
    fn restore_scroll(&mut self, tab: TabId, at: &obs_engine::ScrollPosition) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Some(view) = t.view.as_ref() else {
            return;
        };
        let Some(unit) = view.doc.units.iter().position(|u| u.id == at.unit) else {
            return;
        };
        let anchor = crate::view::TextAnchor {
            unit,
            byte: at.byte,
            into: at.into,
        };
        t.restore_scroll = Some(anchor);
    }

    /// A Tab was opened: load its Resource if it has one.
    fn opened(&mut self, tab: TabId) {
        self.load(tab, TabUi::default(), None);
    }

    /// A Resource Tab now shows another Resource (#151): park where its view was scrolled
    /// and load the new one in a pane as wide, where Back or Forward scrolls it back to
    /// where it was parked.
    fn switched(&mut self, tab: TabId) {
        self.footnotes
            .send(tab, footnotes::FootnoteEvent::ResourceChanged);
        let Some(mut old) = self.tabs.remove(&tab) else {
            return;
        };
        let mut parked = std::mem::take(&mut old.parked);
        let width = old.view.as_ref().map(ReadingView::width);
        if let Some(view) = old.view.as_mut() {
            let at = if matches!(
                view.doc.manifest.resource_type,
                ResourceType::Lexicon | ResourceType::Dictionary
            ) {
                view.current_entry().map(ParkedAt::Entry)
            } else {
                view.current_location().map(ParkedAt::Verse)
            };
            if let Some(at) = at
                && let Some(anchor) = view.text_anchor()
            {
                let resource = view.doc.manifest.resource_id.clone();
                // One place per unit at a location: a book introduction and the section
                // sharing its first Verse are parked apart (#176).
                parked.retain(|p| {
                    p.resource != resource || p.at != at || p.anchor.unit != anchor.unit
                });
                parked.push(ParkedScroll {
                    resource,
                    at,
                    anchor,
                });
                if parked.len() > PARKED_LIMIT {
                    parked.remove(0);
                }
            }
        }
        let ui_state = TabUi {
            parked,
            header_controls: old.header_controls,
            ..TabUi::default()
        };
        self.load(tab, ui_state, width);
    }

    /// Back or Forward returned to a history entry whose view showed `unit` (#176): where the
    /// location alone shows another unit, go to `unit`, where the Tab was parked in it at
    /// this location if it was, else to its start.
    fn show_unit(&mut self, tab: TabId, unit: &obs_model::UnitId) {
        let here = self.session.workspace.tab(tab).and_then(|t| {
            if t.keeps_entries() {
                t.entry_history.current().map(ParkedAt::Entry)
            } else {
                t.history.current().map(ParkedAt::Verse)
            }
        });
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Some(view) = t.view.as_mut() else {
            return;
        };
        let Some(i) = view.doc.units.iter().position(|u| u.id == *unit) else {
            return;
        };
        if view.top_unit() == Some(i) {
            return;
        }
        let resource = &view.doc.manifest.resource_id;
        let parked = t.parked.iter().find(|p| {
            p.resource == *resource && here.as_ref() == Some(&p.at) && p.anchor.unit == i
        });
        let moved = match parked {
            Some(p) => view.restore_text_anchor(p.anchor, p.at.verse()),
            None => view.jump_to_text(i, 0),
        };
        if moved {
            // The location's flash marks a place in the unit the view has left.
            t.flash = None;
        }
    }

    /// Report the text unit at the top of Resource Tab `tab`'s view to the Workspace, which
    /// snapshots it into each history entry it pushes (#176). A Bible's location alone always
    /// shows the unit it is in, so it reports none.
    fn report_view_unit(&mut self, tab: TabId) {
        let ws = &self.session.workspace;
        if !matches!(ws.tab(tab).map(|t| &t.kind), Some(TabKind::Resource(_))) {
            return;
        }
        let unit = self
            .tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .filter(|v| v.doc.manifest.resource_type != ResourceType::Bible)
            .and_then(|v| v.doc.units.get(v.top_unit()?))
            .map(|u| u.id.clone());
        self.session.workspace.view_shows(tab, unit);
    }

    /// Load a Tab's Resource, if it has one, into `ui_state`, laid out `width` wide if known.
    fn load(&mut self, tab: TabId, mut ui_state: TabUi, width: Option<f32>) {
        let kind = self.session.workspace.tab(tab).map(|t| t.kind.clone());
        if kind == Some(TabKind::Tool(Tool::TextComparison)) {
            ui_state.comparison = Some(ComparisonView::new(self.session.text));
        }
        if kind == Some(TabKind::Tool(Tool::Search)) {
            ui_state.search = Some(crate::search_view::SearchView::new(self.session.text));
        }
        if let Some(TabKind::Resource(id)) = &kind {
            match self.doc(id) {
                Ok(doc) => {
                    let mut view = ReadingView::new(Arc::clone(&doc), self.session.text);
                    if let Some(width) = width {
                        view.set_width(width);
                    }
                    if let Some(t) = self.session.workspace.tab_mut(tab) {
                        t.versification = (doc.manifest.resource_type == ResourceType::Bible)
                            .then(|| Arc::new(doc.versification.clone()));
                        if doc.manifest.resource_type != ResourceType::Bible {
                            t.navigation = Some(Arc::clone(&doc.navigation));
                        }
                        t.verse_map = doc.verse_map.clone();
                        if !doc.manifest.key_schemes.is_empty() {
                            t.key_schemes = doc.manifest.key_schemes.clone();
                        }
                    }
                    // A switch to a parallel resource goes to the same place (#151);
                    // otherwise start where the Link set is; unlinked, where another Bible is.
                    let arrived = self.session.workspace.arrival(tab);
                    let ws = &self.session.workspace;
                    let entries = matches!(
                        doc.manifest.resource_type,
                        ResourceType::Lexicon | ResourceType::Dictionary
                    );
                    let own = ws.tab(tab).and_then(|t| t.history.current());
                    let at = arrived
                        .or(own)
                        .or_else(|| ws.set_location(tab))
                        .or_else(|| {
                            ws.tabs()
                                .filter(|t| {
                                    t.id != tab && t.kind.is_reading() && t.verse_map.is_none()
                                })
                                .find_map(|t| t.history.current())
                        });
                    if let Some(v) = at {
                        let local = if doc.manifest.resource_type == ResourceType::Commentary {
                            doc.navigation.section(v).and_then(|e| match e.key {
                                Key::Passage(p) => Some(p.start()),
                                _ => None,
                            })
                        } else {
                            Some(v)
                        };
                        if let Some(v) = local
                            && view.jump_to(v)
                        {
                            self.session.workspace.scrolled(tab, v);
                            if arrived.is_some() {
                                ui_state.flash = Some((crate::pane::Flash::Verse(v), self.now));
                            }
                        }
                    }
                    if entries {
                        let ws = &self.session.workspace;
                        // Another Lexicon or Dictionary's Key (a parallel resource, #151) is
                        // looked up in this one.
                        let key = ws
                            .tab(tab)
                            .and_then(|t| t.entry_history.current())
                            .and_then(|k| doc.navigation.lookup(&k).map(|e| e.key.clone()))
                            .or_else(|| ws.set_entry_location(tab))
                            .or_else(|| doc.navigation.entries().next().map(|e| e.key.clone()));
                        if let Some(key) = key
                            && view.jump_to_entry(&key)
                            && let Some(t) = self.session.workspace.tab_mut(tab)
                        {
                            t.entry_history.scrolled_to(key);
                        }
                    }
                    // Back or Forward to a place the Tab switched away from (#151) scrolls
                    // to where it was there.
                    let ws = &self.session.workspace;
                    let here = ws.tab(tab).and_then(|t| {
                        if entries {
                            t.entry_history.current().map(ParkedAt::Entry)
                        } else {
                            t.history.current().map(ParkedAt::Verse)
                        }
                    });
                    // Where the Tab was parked at this location counts only in the unit the
                    // location alone shows (#176): another unit parked at the same Verse or Key
                    // (a book introduction, the section sharing its start) is for Back or
                    // Forward to a history entry that showed it ([`Effect::ShowUnit`]).
                    let showing = (doc.manifest.resource_type != ResourceType::Bible)
                        .then(|| view.top_unit())
                        .flatten();
                    if arrived.is_none()
                        && let Some(p) = ui_state.parked.iter().find(|p| {
                            here.as_ref() == Some(&p.at)
                                && p.resource == *id
                                && showing.is_none_or(|i| p.anchor.unit == i)
                        })
                    {
                        view.restore_text_anchor(p.anchor, p.at.verse());
                    }
                    ui_state.view = Some(view);
                }
                Err(e) => ui_state.error = Some(e),
            }
        }
        if let Some(TabKind::About(id)) = &kind
            && let Some(e) = self.session.library.get(id)
        {
            let doc = ReadingDoc::from_text(e.manifest.clone(), &e.manifest.attribution);
            ui_state.view = Some(ReadingView::new(Arc::new(doc), self.session.text));
        }
        self.tabs.insert(tab, ui_state);
        if let Some(standard) = self.references.pending_navigation.remove(&tab) {
            self.session.workspace.navigate_standard(tab, standard);
        }
    }

    fn doc(&mut self, id: &str) -> Result<Arc<ReadingDoc>, String> {
        if let Some(d) = self.docs.get(id) {
            return Ok(Arc::clone(d));
        }
        let entry = self
            .session
            .library
            .get(id)
            .ok_or_else(|| format!("{id} is not in the library"))?;
        let doc = load(&entry.path)?;
        self.docs.insert(id.to_string(), Arc::clone(&doc));
        Ok(doc)
    }

    /// The location shown in a Tab's reference box, in the Tab's own numbering.
    fn location_text(&self, tab: TabId) -> String {
        let Some(t) = self.session.workspace.tab(tab) else {
            return String::new();
        };
        if t.header_box() == HeaderBox::Word
            && let Some(words) = self.session.word_study.choosing(tab)
        {
            return words.to_string();
        }
        if t.header_box() == HeaderBox::Name {
            if let Some(name) = self.session.factbooks.choosing(tab) {
                return name.to_string();
            }
            // The entity's name, not its id.
            let Some(obs_model::Key::Headword(id)) = t.entry_history.current() else {
                return String::new();
            };
            return self
                .session
                .guides
                .directory()
                .current(&self.session.library)
                .and_then(|d| d.card(&id))
                .map_or_else(|| obs_engine::people_places::display_name(&id), |c| c.name);
        }
        if t.header_box() == HeaderBox::Topic
            && let Some(text) = self.session.topic_guides.choosing(tab)
        {
            return text.to_string();
        }
        if matches!(
            t.header_box(),
            HeaderBox::Lexicon | HeaderBox::Headword | HeaderBox::Word | HeaderBox::Topic
        ) {
            return t
                .entry_history
                .current()
                .map(|k| k.to_string())
                .unwrap_or_default();
        }
        let Some(v) = t.history.current() else {
            return String::new();
        };
        // A Guide or Tool at a range or chapter shows all of it (Standard numbering): its first
        // Verse alone narrowed the Guide when the box was confirmed (#261).
        if let Some(p) = t.shown_passage().filter(|p| !p.is_single_verse()) {
            return format_passage(p, &Standard, RefStyle::Long);
        }
        let p = Passage::verse(v);
        match &t.versification {
            Some(own) => format_passage(p, own.as_ref(), RefStyle::Long),
            None => format_passage(p, &Standard, RefStyle::Long),
        }
    }

    fn tab_body(&mut self, ui: &mut Ui, tab: TabId) {
        let pane = ui.max_rect();
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(pane) {
            self.session.workspace.user_focused(tab);
        }
        let Some(kind) = self.session.workspace.tab(tab).map(|t| t.kind.clone()) else {
            return;
        };
        self.header(ui, tab, &kind);
        if self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| matches!(t.header_box(), HeaderBox::Lexicon | HeaderBox::Headword))
        {
            self.entry_buttons(ui, tab);
        }
        // The body gets exactly the rest of the pane, whatever the header did (#13 finding).
        let rect = egui::Rect::from_min_max(pos2(pane.left(), ui.cursor().top()), pane.max);
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("body", tab)));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        match kind {
            TabKind::Resource(_) => self.reading_body(&mut body, tab),
            TabKind::Library => self.library_body(&mut body, tab),
            TabKind::About(id) => self.about_body(&mut body, tab, &id),
            TabKind::ImportReport(id) => self.import_body(&mut body, &id),
            TabKind::GetBibles => self.get_bibles_body(&mut body),
            TabKind::StarterLibrary => self.starter_body(&mut body),
            TabKind::Credits => self.credits_body(&mut body, tab),
            TabKind::Home => self.home_body(&mut body, tab),
            TabKind::KeyboardShortcuts => self.keys_body(&mut body, tab),
            TabKind::Settings => self.settings_body(&mut body, tab),
            TabKind::Backups => self.backups_body(&mut body, tab),
            TabKind::Guide(
                g @ (obs_engine::Guide::PassageGuide | obs_engine::Guide::ExegeticalGuide),
            ) => self.passage_keyed_guide_body(&mut body, tab, g),
            TabKind::Guide(obs_engine::Guide::WordStudy) => self.word_study_body(&mut body, tab),
            TabKind::Guide(obs_engine::Guide::Factbook) => self.factbook_body(&mut body, tab),
            TabKind::Guide(obs_engine::Guide::TopicGuide) => {
                self.topic_guide_body(&mut body, tab);
            }
            TabKind::Tool(Tool::TextComparison) => self.comparison_body(&mut body, tab),
            TabKind::Tool(Tool::CrossReferences) => self.cross_references_body(&mut body, tab),
            TabKind::Tool(Tool::WordInfo) => self.word_info_body(&mut body, tab),
            TabKind::Tool(Tool::Search) => self.search_body(&mut body, tab),
            TabKind::Tool(Tool::Notes) => self.notes_body(&mut body, tab),
            TabKind::Tool(Tool::Favorites) => self.favorites_body(&mut body, tab),
            TabKind::Tool(Tool::History) => self.history_body(&mut body, tab),
            TabKind::Tool(Tool::ReadingPlans) => self.reading_plans_body(&mut body, tab),
            TabKind::Tool(Tool::Atlas) => self.atlas_body(&mut body, tab),
            TabKind::Tool(Tool::Timeline) => self.timeline_body(&mut body, tab),
        }
    }

    /// A Resource Tab's text, with its Contents pane beside it when the pane is open (#176).
    fn reading_body(&mut self, ui: &mut Ui, tab: TabId) {
        let open = self.session.workspace.tab(tab).is_some_and(|t| t.contents);
        if !open {
            self.reading_text(ui, tab);
            return;
        }
        let rect = ui.max_rect();
        let width = contents_pane::pane_width(rect.width());
        let side = egui::Rect::from_min_max(rect.min, pos2(rect.left() + width, rect.bottom()));
        let text = egui::Rect::from_min_max(pos2(side.right(), rect.top()), rect.max);
        let mut body = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text)
                .id_salt(("reading", tab)),
        );
        body.set_clip_rect(text.intersect(ui.clip_rect()));
        self.reading_text(&mut body, tab);
        // Drawn after the text, so the highlight is where the text is now.
        let mut pane = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(side)
                .id_salt(("contents", tab)),
        );
        pane.set_clip_rect(side.intersect(ui.clip_rect()));
        self.contents_pane(&mut pane, tab);
    }

    fn reading_text(&mut self, ui: &mut Ui, tab: TabId) {
        // The Tab switched Resources this frame (#151) and its view still shows the old one
        // until the switch is applied: the old view must not report its location.
        let stale = match (
            self.session.workspace.tab(tab).map(|t| &t.kind),
            self.tabs.get(&tab).and_then(|t| t.view.as_ref()),
        ) {
            (Some(TabKind::Resource(id)), Some(view)) => view.doc.manifest.resource_id != *id,
            _ => false,
        };
        if stale {
            ui.ctx().request_repaint();
            return;
        }
        self.sync_interlinear(tab, false);
        self.interlinear_hint(ui, tab);
        let filters = self.filter_source(tab);
        let palette = self.palette;
        // The Tab's own Reading layout over the defaults (#209). A change lays the text out
        // again, keeping the Current location.
        let mut text = obs_layout::TextSettings {
            layout: self.session.reading_layout(tab),
            ..self.session.text
        };
        text.footnote_markers = self
            .session
            .workspace
            .tab(tab)
            .is_none_or(|t| t.footnote_markers);
        let restoring_at = (self.session.workspace.tab(tab))
            .filter(|t| !t.keeps_entries())
            .and_then(|t| t.history.current());
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Some(view) = t.view.as_mut() else {
            let msg = t.error.clone().unwrap_or_default();
            ui.centered_and_justified(|ui| ui.colored_label(theme::c().warn, msg));
            return;
        };
        view.set_settings(text);
        // The flash fades out; with motion reduced it holds, then goes at once (#225).
        let reduce = crate::motion::reduced(ui.ctx());
        let age = t
            .flash
            .as_ref()
            .map_or(motion::FLASH, |(_, start)| self.now - start);
        let flash = (t.flash.clone())
            .filter(|_| age < motion::FLASH)
            .map(|(v, _)| {
                let fade = motion::remaining(if reduce { 0.0 } else { age }, 0.0, motion::FLASH);
                (v, palette.flash.gamma_multiply(fade))
            });
        if flash.is_none() {
            t.flash = None;
        } else if reduce {
            let left = std::time::Duration::from_secs_f64(motion::FLASH - age);
            ui.ctx().request_repaint_after(left);
        } else {
            ui.ctx().request_repaint();
        }
        // The Find bar (#175) moves the view before the pane draws, like a jump.
        let viewport = ui.available_rect_before_wrap().height();
        let found_moved = t
            .find
            .as_mut()
            .is_some_and(|bar| bar.update_reading(view, viewport, ui.ctx()));
        let found = t
            .find
            .as_ref()
            .map(crate::find::FindBar::found)
            .unwrap_or_default();
        let marks = self.marks.marks(&self.session, &view.doc);
        // The matches are for these filters in this document.
        let doc_key = Arc::as_ptr(&view.doc) as usize as u64;
        t.filters
            .set(filters.map(|(key, source)| (key ^ doc_key.rotate_left(17), source)));
        let pane_marks = crate::pane::PaneMarks {
            annotations: &marks,
            filters: Some(&mut t.filters),
        };
        let out = reading_pane(
            ui,
            view,
            &mut self.atlas,
            &palette,
            flash,
            pane_marks,
            found,
        );
        let user_moved = out.user_moved || found_moved;
        // A reopened Tab's scroll position (#214) goes back once the pane has drawn at its
        // width; the view keeps it across later relayouts until the user scrolls or it jumps.
        if let Some(anchor) = t.restore_scroll.take()
            && !user_moved
        {
            view.restore_text_anchor(anchor, restoring_at);
            ui.ctx().request_repaint();
        }
        let following = view.is_following();
        let at = view.current_location();
        let reference = out
            .hovered_reference
            .map(|(unit, span)| references::TargetKey::new(&view.doc, unit, span));
        if let Some(n) = out.hovered_verse_number {
            verse_number_tooltip(ui, &view.doc, n);
        }
        let entry = user_moved.then(|| view.current_entry()).flatten();
        self.word_hovered(ui, tab, out.hovered_word.clone(), out.clicked_word.clone());
        if let Some(key) = entry {
            self.session.workspace.user_scrolled_entry(tab, key);
        }
        self.report_view_unit(tab);
        self.report_scroll(tab);
        if let Some(at) = at {
            let ws = &mut self.session.workspace;
            if user_moved {
                ws.user_scrolled(tab, at);
            } else if !following && ws.tab(tab).is_some_and(|t| t.history.current() != Some(at)) {
                ws.scrolled(tab, at);
            }
        }
        if let Some(key) = reference {
            if out.reference_clicked {
                self.references.pending_clicks.insert(
                    tab,
                    (
                        key.clone(),
                        ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
                    ),
                );
            }
            match self.references.targets.get(key) {
                Some(Ok(targets)) => self.reference_popup(ui, &targets),
                Some(Err(error)) => self.reference_message_popup(ui, &error),
                None => {
                    self.reference_message_popup(ui, "Loading Scripture reference…");
                    ui.ctx().request_repaint();
                }
            }
        }
        // A click on a cold cache must not disappear when the pointer leaves the span.
        if let Some((key, new_tab)) = self.references.pending_clicks.get(&tab).cloned() {
            match self.references.targets.get(key) {
                None => ui.ctx().request_repaint(),
                Some(result) => {
                    self.references.pending_clicks.remove(&tab);
                    match result {
                        Ok(targets) => {
                            if let Some(passage) = targets.iter().flat_map(|p| p.iter()).next() {
                                self.reference_navigate(tab, *passage, new_tab);
                            }
                        }
                        Err(error) => self.toast(error),
                    }
                }
            }
        }
        self.footnote_frame(ui, tab, &out);
        if let Some(drag) = out.drag_out {
            self.text_drag_started(ui, tab, drag);
        }
        self.after_reading_pane(ui, tab, &out);
    }

    /// What Tab `tab`'s Visual filters (#211) are matched with, and a key that changes when
    /// any of it does: none when the Tab shows no filter.
    fn filter_source(&self, tab: TabId) -> Option<(u64, crate::filters::FilterSource)> {
        use std::hash::{Hash, Hasher};
        let filters = self.session.tab_visual_filters(tab);
        if filters.is_empty() {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.session.visual_filters_revision().hash(&mut h);
        for f in &filters {
            f.id.hash(&mut h);
        }
        let library = &self.session.library;
        let mut lexicons = Vec::new();
        for e in library.of_type(ResourceType::Lexicon) {
            (&e.manifest.resource_id, e.manifest.revision).hash(&mut h);
            lexicons.push(e.path.clone());
        }
        let mut originals = Vec::new();
        for e in library.of_type(ResourceType::Bible) {
            let language = [
                obs_scripture::StrongsLanguage::Hebrew,
                obs_scripture::StrongsLanguage::Greek,
            ]
            .into_iter()
            .find(|l| obs_engine::key_words::is_original_language(&e.manifest.language, *l));
            let Some(language) = language else {
                continue;
            };
            (&e.manifest.resource_id, e.manifest.revision).hash(&mut h);
            originals.push(crate::filters::OriginalBible::new(
                language,
                Some(e.path.clone()),
                self.docs.get(&e.manifest.resource_id).cloned(),
            ));
        }
        Some((
            h.finish(),
            crate::filters::FilterSource::new(filters, lexicons, originals),
        ))
    }

    /// The interlinear kind and lines (#143) a Bible Tab shows: none when they are off, or
    /// when its Bible has no Tagged words.
    fn interlinear_lines(&self, tab: TabId) -> Option<(InterlinearKind, Vec<InterlinearLine>)> {
        let kind = obs_engine::interlinear::tab_kind(&self.session, tab)?;
        let lines = self.session.workspace.tab(tab)?.interlinear.shown(kind);
        let view = self.tabs.get(&tab)?.view.as_ref()?;
        (!lines.is_empty() && view.doc.has_tagged_words()).then_some((kind, lines))
    }

    /// Give a Bible Tab's view the interlinear lines the Tab shows (#143). They are built
    /// again only when the lines, or the Lexicons they are looked up in (or their
    /// revisions), change, or when `rebuild` (a Lexicon was replaced in place).
    fn sync_interlinear(&mut self, tab: TabId, rebuild: bool) {
        let wanted = self.interlinear_lines(tab);
        let lexicons: Vec<(String, u32)> = match &wanted {
            Some((kind, lines)) if InterlinearSource::needs_lexicons(*kind, lines) => self
                .session
                .library
                .of_type(ResourceType::Lexicon)
                .map(|e| (e.manifest.resource_id.clone(), e.manifest.revision))
                .collect(),
            _ => Vec::new(),
        };
        let Some(view) = self.tabs.get(&tab).and_then(|t| t.view.as_ref()) else {
            return;
        };
        let current = view.interlinear();
        let source = match wanted {
            None if current.is_none() => return,
            None => None,
            Some((kind, lines)) => {
                if !rebuild && current.is_some_and(|s| s.is_for(kind, &lines, &lexicons)) {
                    return;
                }
                // A Lexicon that cannot be loaded is left out (its Tab would say why).
                let docs = lexicons
                    .iter()
                    .filter_map(|(id, _)| self.doc(id).ok())
                    .collect();
                InterlinearSource::new(kind, lines, lexicons, docs).map(Arc::new)
            }
        };
        if let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) {
            view.set_interlinear(source);
        }
    }

    /// The hint over a Bible Tab's text when some of the interlinear lines it shows have shown
    /// nothing under any word (#243): what is missing, the buttons that would fill it (#135),
    /// and Close, which keeps it closed on this Tab.
    fn interlinear_hint(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(kind) = obs_engine::interlinear::tab_kind(&self.session, tab) else {
            return;
        };
        if self
            .session
            .workspace
            .tab(tab)
            .is_none_or(|t| !t.interlinear.on || t.interlinear.hint_dismissed)
        {
            return;
        }
        let empty = (self.tabs.get(&tab))
            .and_then(|t| t.view.as_ref())
            .and_then(|v| v.interlinear())
            .map(|source| source.empty_lines())
            .unwrap_or_default();
        let Some(hint) = obs_engine::interlinear::missing_lines_hint(&self.session, kind, &empty)
        else {
            return;
        };
        let mut action = None;
        let mut close = false;
        crate::components::card_frame()
            .corner_radius(crate::tokens::corner(radius::S))
            .inner_margin(inset::NOTICE)
            .outer_margin(egui::Margin {
                bottom: space::S as i8,
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // Room for the Close button beside the message.
                    let width = (ui.available_width() - size::CONTROL).max(size::FIELD_NARROW);
                    ui.vertical(|ui| {
                        ui.set_max_width(width);
                        action = self.missing_resource_ui(ui, hint.message, &hint.missing);
                    });
                    close = icons::button(
                        ui,
                        icons::Icon::Close,
                        icons::SMALL,
                        theme::c().muted,
                        "Close this hint",
                    )
                    .on_hover_text("Close this hint (it stays closed on this Tab)")
                    .clicked();
                });
            });
        if close {
            self.run(
                "tab.interlinear",
                &json!({ "tab": tab.0, "dismiss_hint": true }),
            );
        }
        if let Some((id, params)) = action {
            self.run(id, &params);
        }
    }

    /// Look a replaced Lexicon's words up again in every Tab whose interlinear lines use it
    /// (#143): the update keeps its resource id, and maybe its revision.
    fn lexicon_replaced(&mut self, resource_id: &str) {
        let using: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|(_, t)| {
                t.view
                    .as_ref()
                    .and_then(|v| v.interlinear())
                    .is_some_and(|s| s.looks_up_in(resource_id))
            })
            .map(|(id, _)| *id)
            .collect();
        for tab in using {
            self.sync_interlinear(tab, true);
        }
    }

    /// A Bible Tab's interlinear controls (#143), laid out right to left: the lines menu (⏷)
    /// and the Interlinear toggle. Only for a Bible with Tagged words.
    fn interlinear_controls(&mut self, ui: &mut Ui, tab: TabId, narrow: bool) {
        let Some(kind) = obs_engine::interlinear::tab_kind(&self.session, tab) else {
            return;
        };
        if !self
            .tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .is_some_and(|v| v.doc.has_tagged_words())
        {
            return;
        }
        let Some(lines) = self
            .session
            .workspace
            .tab(tab)
            .map(|t| t.interlinear.clone())
        else {
            return;
        };
        let what = match kind {
            InterlinearKind::Reverse => "the Hebrew or Greek under each word",
            InterlinearKind::Interlinear => "English and grammar under each word",
        };
        let mut run: Option<Value> = None;
        crate::components::menu(
            ui,
            icons::image(
                ui.ctx(),
                icons::Icon::ChevronDown,
                icons::SMALL,
                theme::c().muted,
            ),
            |ui| {
                ui.label(crate::components::hint_text("Interlinear lines"));
                for line in kind.lines() {
                    let mut chosen = lines.is_chosen(kind, *line);
                    // The last line picked stays.
                    let last = chosen && lines.shown(kind).len() <= 1 && lines.on;
                    if ui
                        .add_enabled(!last, egui::Checkbox::new(&mut chosen, line.name()))
                        .clicked()
                    {
                        run = Some(json!({ "tab": tab.0, "line": line.id(), "shown": chosen }));
                    }
                }
            },
        )
        .response
        .on_hover_text("Choose the interlinear lines");
        let label = if narrow { "Int." } else { "Interlinear" };
        if ui
            .selectable_label(lines.on, RichText::new(label).small())
            .on_hover_text(format!("Interlinear lines: {what}"))
            .clicked()
        {
            run = Some(json!({ "tab": tab.0, "on": !lines.on }));
        }
        if let Some(params) = run {
            self.session.workspace.user_focused(tab);
            self.run("tab.interlinear", &params);
        }
    }

    /// Text Comparison: the Bible chips, then the Verse blocks. The view follows the Tab's
    /// location (moved by its Link set or reference box) and scrolling it moves the Link set.
    fn comparison_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(t) = self.session.workspace.tab(tab) else {
            return;
        };
        let comparison = t.comparison.clone();
        let location = t.history.current();
        let bibles = comparison.bibles(&self.session.library);
        let base = comparison.base_index(&bibles);
        let Some(tab_ui) = self.tabs.get_mut(&tab) else {
            return;
        };
        let mut errors = std::mem::take(&mut tab_ui.comparison_errors);
        errors.retain(|id, _| bibles.contains(id));
        let mut docs = Vec::new();
        for id in &bibles {
            let result = match errors.get(id) {
                Some(error) => Err(error.clone()),
                None => self.doc(id),
            };
            if let Err(error) = &result {
                errors.insert(id.clone(), error.clone());
            }
            if let Some(entry) = self.session.library.get(id) {
                docs.push(ComparisonBible {
                    id: id.clone(),
                    abbreviation: entry.manifest.abbreviation.clone(),
                    language: entry.manifest.language.clone(),
                    doc: result,
                });
            }
        }
        let extent = location.map(|v| comparison.extent(v)).unwrap_or_default();
        self.comparison_chips(ui, tab, &comparison, &bibles, base);
        if !errors.is_empty() && ui.button("Retry unavailable Bibles").clicked() {
            errors.clear();
        }
        let palette = self.palette;
        let Some(tab_ui) = self.tabs.get_mut(&tab) else {
            return;
        };
        tab_ui.comparison_errors = errors;
        let Some(view) = tab_ui.comparison.as_mut() else {
            return;
        };
        view.set_settings(self.session.text);
        view.set(docs, base, comparison.differences, extent);
        if location != view.synced {
            if let Some(v) = location {
                view.jump_to(v);
            }
            view.synced = location;
        }
        let found_moved = tab_ui
            .find
            .as_mut()
            .is_some_and(|bar| bar.update_comparison(view, ui.ctx()));
        let found = tab_ui
            .find
            .as_ref()
            .map(crate::find::FindBar::found)
            .unwrap_or_default();
        let out = comparison_pane(ui, view, &mut self.atlas, &palette, found);
        if (out.user_moved || found_moved || out.find_moved)
            && let Some(v) = view.current_location()
            && Some(v) != view.synced
        {
            view.synced = Some(v);
            self.session.workspace.user_scrolled(tab, v);
        }
        self.comparison_menu(ui, tab, out.response.as_ref(), out.context.as_ref());
    }

    /// The chips under a Text Comparison header: one per Bible (☆ makes it the ★ base, ×
    /// removes it), + Bible, Show differences, and the typed range's removable chip.
    fn comparison_chips(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        comparison: &obs_engine::Comparison,
        bibles: &[String],
        base: Option<usize>,
    ) {
        let mut run: Option<(&'static str, Value)> = None;
        let abbrev = |id: &str| {
            self.session
                .library
                .get(id)
                .map_or_else(|| id.to_string(), |e| e.manifest.abbreviation.clone())
        };
        let others: Vec<(String, String)> = self
            .session
            .library
            .of_type(ResourceType::Bible)
            .filter(|e| !bibles.contains(&e.manifest.resource_id))
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                )
            })
            .collect();
        egui::Frame::new()
            .fill(theme::c().surface)
            .inner_margin(inset::BAR)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    for (i, id) in bibles.iter().enumerate() {
                        crate::components::chip_frame().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::XXS;
                                let (rect, r) = ui.allocate_exact_size(
                                    vec2(size::ICON, size::CONTROL_COMPACT),
                                    egui::Sense::click(),
                                );
                                let color = if r.hovered() || Some(i) == base {
                                    theme::c().lapis
                                } else {
                                    theme::c().muted
                                };
                                icons::paint(
                                    ui.painter(),
                                    if Some(i) == base {
                                        icons::Icon::StarFilled
                                    } else {
                                        icons::Icon::Star
                                    },
                                    rect,
                                    icons::SMALL,
                                    color,
                                );
                                if r.on_hover_text("Compare the others with this Bible")
                                    .clicked()
                                {
                                    run = Some(("compare.base", json!({ "tab": tab.0, "id": id })));
                                }
                                ui.label(crate::components::label_text(abbrev(id)));
                                let remove = icons::button(
                                    ui,
                                    icons::Icon::Close,
                                    icons::SMALL,
                                    theme::c().muted,
                                    "Remove",
                                );
                                if remove.on_hover_text("Remove").clicked() {
                                    run =
                                        Some(("compare.bible", json!({ "tab": tab.0, "id": id })));
                                }
                            });
                        });
                    }
                    crate::components::menu(ui, crate::components::label_text("+ Bible"), |ui| {
                        if others.is_empty() {
                            ui.label(RichText::new("Every Bible is shown").color(theme::c().muted));
                        }
                        for (id, label) in &others {
                            if ui.button(label).clicked() {
                                run = Some(("compare.bible", json!({ "tab": tab.0, "id": id })));
                                ui.close();
                            }
                        }
                    });
                    let mut differences = comparison.differences;
                    if ui
                        .checkbox(
                            &mut differences,
                            crate::components::label_text("Differences"),
                        )
                        .clicked()
                    {
                        run = Some(("compare.differences", json!({ "tab": tab.0 })));
                    }
                    if let Some(range) = comparison.range {
                        let text = format_passage(range, &Standard, RefStyle::Short);
                        if ui
                            .add(
                                crate::components::button((
                                    crate::components::label_text(text),
                                    icons::image(
                                        ui.ctx(),
                                        icons::Icon::Close,
                                        icons::SMALL,
                                        theme::c().muted,
                                    ),
                                ))
                                .corner_radius(radius::PILL),
                            )
                            .on_hover_text("Show the whole chapter")
                            .clicked()
                        {
                            run = Some(("compare.clear_range", json!({ "tab": tab.0 })));
                        }
                    }
                });
            });
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }

    fn header(&mut self, ui: &mut Ui, tab: TabId, kind: &TabKind) {
        let focused = self.session.workspace.focused() == Some(tab);
        let fill = if focused {
            theme::c().field
        } else {
            theme::c().surface
        };
        let frame = egui::Frame::new()
            .fill(fill)
            .inner_margin(inset::BAR)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    let narrow = ui.available_width() < size::HEADER_NARROW;
                    ui.add_space(space::XS);
                    if kind.has_link_chip() {
                        self.link_chip(ui, tab);
                    }
                    if matches!(kind, TabKind::Resource(_)) {
                        self.contents_button(ui, tab);
                    }
                    let header = self
                        .session
                        .workspace
                        .tab(tab)
                        .map_or(kind.header_box(), |t| t.header_box());
                    if matches!(
                        header,
                        HeaderBox::Reference
                            | HeaderBox::Lexicon
                            | HeaderBox::Headword
                            | HeaderBox::Word
                            | HeaderBox::Name
                            | HeaderBox::Topic
                    ) {
                        self.history_buttons(ui, tab);
                    }
                    // The box leaves room for the controls on the right (measured last frame).
                    let controls = self.tabs.get(&tab).map_or(0.0, |t| t.header_controls);
                    let room = ui.available_width() - controls - 2.0 * ui.spacing().item_spacing.x;
                    let width = room
                        .min(if narrow {
                            size::HEADER_BOX_NARROW
                        } else {
                            size::HEADER_BOX
                        })
                        .max(size::FIELD_NUMBER);
                    self.header_box(ui, tab, header, width);
                    let title = self.title(tab, kind);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.tab_menu(ui, tab, narrow);
                        self.favorite_star(ui, tab);
                        if matches!(kind, TabKind::Resource(_)) {
                            if !narrow {
                                self.filters_menu(ui, tab);
                            }
                            self.interlinear_controls(ui, tab, narrow);
                            self.parallel_resources(ui, tab, narrow);
                        }
                        let measured = ui.min_rect().width();
                        if let Some(t) = self.tabs.get_mut(&tab)
                            && (t.header_controls - measured).abs() > 0.5
                        {
                            t.header_controls = measured;
                            ui.ctx().request_repaint();
                        }
                        if !narrow {
                            ui.add(
                                egui::Label::new(crate::components::hint_text(title)).truncate(),
                            );
                        }
                    });
                });
                if let Some(e) = self.tabs.get(&tab).and_then(|t| t.error.clone()) {
                    ui.label(RichText::new(e).small().color(theme::c().warn));
                }
                if kind.has_find_bar() && self.tabs.get(&tab).is_some_and(|t| t.find.is_some()) {
                    self.find_row(ui, tab);
                }
            });
        let r = frame.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                r.min,
                vec2(if focused { space::XS } else { space::XXS }, r.height()),
            ),
            0.0,
            theme::link_color(self.session.workspace.tab(tab).and_then(|t| t.link)),
        );
        ui.painter().hline(
            r.x_range(),
            r.bottom() - stroke::HAIR / 2.0,
            crate::components::rule_stroke(),
        );
    }

    fn title(&self, tab: TabId, kind: &TabKind) -> String {
        match kind {
            TabKind::Resource(id) => self
                .session
                .library
                .get(id)
                .map(|e| e.manifest.title.clone())
                .unwrap_or_default(),
            _ => self
                .session
                .workspace
                .tab(tab)
                .map(|t| t.title.clone())
                .unwrap_or_default(),
        }
    }

    /// The parallel-resources control of a Resource Tab (#151), laid out right to left:
    /// ◀, a dropdown of the Resources of the Tab's Resource type in Preferred order, ▶;
    /// compact in a `narrow` Panel.
    fn parallel_resources(&mut self, ui: &mut Ui, tab: TabId, narrow: bool) {
        let current = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Resource(id)) => id.clone(),
            _ => return,
        };
        let parallel: Vec<(String, String, String)> =
            obs_engine::parallel_resources(&self.session, tab)
                .iter()
                .map(|e| {
                    (
                        e.manifest.resource_id.clone(),
                        e.manifest.abbreviation.clone(),
                        e.manifest.title.clone(),
                    )
                })
                .collect();
        let others = parallel.len() > 1;
        let what = self.session.library.get(&current).map_or("Resources", |e| {
            obs_engine::library_search::type_plural(e.manifest.resource_type)
        });
        let ctx = ui.ctx().clone();
        let arrow = |icon| {
            crate::components::button(icons::image(&ctx, icon, icons::BODY, theme::c().muted))
                .frame(false)
                .min_size(vec2(size::ICON_LARGE, size::CONTROL_COMPACT))
        };
        let mut run: Option<(&'static str, Value)> = None;
        let next = ui
            .add_enabled(others, arrow(icons::Icon::ChevronRight))
            .accessible_name(format!("Next of your {what}"))
            .on_hover_text(with_key(
                &self.session,
                &format!("Next of your {what}"),
                "tab.parallel_next",
            ))
            .on_disabled_hover_text(format!("There are no other {what} in the library"));
        if next.clicked() {
            run = Some(("tab.parallel_next", json!({ "tab": tab.0 })));
        }
        let shown = parallel
            .iter()
            .find(|(id, ..)| *id == current)
            .map_or(current.clone(), |(_, abbreviation, _)| abbreviation.clone());
        ui.add_enabled_ui(others, |ui| {
            egui::ComboBox::from_id_salt(("parallel", tab))
                .selected_text(RichText::new(shown).small())
                .width(if narrow {
                    size::HEADER_CHOOSER_NARROW
                } else {
                    size::HEADER_CHOOSER
                })
                .truncate()
                .show_ui(ui, |ui| {
                    for (id, abbreviation, title) in &parallel {
                        let label = format!("{abbreviation} — {title}");
                        if ui.selectable_label(*id == current, label).clicked() && *id != current {
                            run = Some(("tab.parallel", json!({ "tab": tab.0, "id": id })));
                        }
                    }
                })
                .response
                .on_hover_text(format!("Show another of your {what} here"))
                .on_disabled_hover_text(format!("There are no other {what} in the library"));
        });
        let previous = ui
            .add_enabled(others, arrow(icons::Icon::ChevronLeft))
            .accessible_name(format!("Previous of your {what}"))
            .on_hover_text(with_key(
                &self.session,
                &format!("Previous of your {what}"),
                "tab.parallel_previous",
            ))
            .on_disabled_hover_text(format!("There are no other {what} in the library"));
        if previous.clicked() {
            run = Some(("tab.parallel_previous", json!({ "tab": tab.0 })));
        }
        if let Some((id, params)) = run {
            self.session.workspace.user_focused(tab);
            self.run(id, &params);
        }
    }

    fn entry_buttons(&mut self, ui: &mut Ui, tab: TabId) {
        let enabled = |forward| {
            self.session.workspace.tab(tab).is_some_and(|t| {
                t.entry_history.current().is_some_and(|key| {
                    t.navigation
                        .as_ref()
                        .and_then(|n| n.adjacent(&key, forward))
                        .is_some()
                })
            })
        };
        let previous = enabled(false);
        let next = enabled(true);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(previous, crate::components::button("Previous entry"))
                .clicked()
            {
                self.run("tab.previous_entry", &json!({ "tab": tab.0 }));
            }
            if ui
                .add_enabled(next, crate::components::button("Next entry"))
                .clicked()
            {
                self.run("tab.next_entry", &json!({ "tab": tab.0 }));
            }
        });
    }

    fn history_buttons(&mut self, ui: &mut Ui, tab: TabId) {
        let (back, fwd) = self.session.workspace.tab(tab).map_or((false, false), |t| {
            if t.keeps_entries() {
                (
                    t.entry_history.can_go_back(),
                    t.entry_history.can_go_forward(),
                )
            } else {
                (t.history.can_go_back(), t.history.can_go_forward())
            }
        });
        let ctx = ui.ctx().clone();
        let arrow = |icon| {
            crate::components::button(icons::image(&ctx, icon, icons::BODY, theme::c().ink))
                .frame(false)
                .min_size(vec2(size::CONTROL_COMPACT, size::ROW_COMPACT))
        };
        if ui
            .add_enabled(back, arrow(icons::Icon::Back))
            .accessible_name("Back")
            .on_hover_text(with_key(&self.session, "Back", "tab.back"))
            .clicked()
        {
            self.run("tab.back", &json!({ "tab": tab.0 }));
        }
        if ui
            .add_enabled(fwd, arrow(icons::Icon::Forward))
            .accessible_name("Forward")
            .on_hover_text(with_key(&self.session, "Forward", "tab.forward"))
            .clicked()
        {
            self.run("tab.forward", &json!({ "tab": tab.0 }));
        }
    }

    fn header_box(&mut self, ui: &mut Ui, tab: TabId, kind: HeaderBox, width: f32) {
        match kind {
            HeaderBox::Keyless => {}
            HeaderBox::FollowsHover => {
                if self.session.word_info.pinned {
                    let r = ui.add(
                        crate::components::button((
                            icons::image(ui.ctx(), icons::Icon::Pin, icons::SMALL, theme::c().ink),
                            RichText::new("Pinned").small(),
                        ))
                        .corner_radius(radius::PILL),
                    );
                    if r.on_hover_text(
                        "Pinned to the clicked word. Click (or press Esc) to follow hover again.",
                    )
                    .clicked()
                    {
                        self.run("word_info.unpin", &json!({}));
                    }
                } else {
                    ui.label(crate::components::hint_text("Follows hover"));
                }
            }
            HeaderBox::Filter => {
                // The Settings Tab's box searches its Settings (#213), and has the keyboard
                // when the Tab is focused, so Ctrl+, opens it ready to type.
                let settings =
                    (self.session.workspace.tab(tab)).is_some_and(|t| t.kind == TabKind::Settings);
                let name = if settings {
                    "Search settings"
                } else {
                    "Filter"
                };
                let focused = settings && self.session.workspace.focused() == Some(tab);
                if let Some(t) = self.tabs.get_mut(&tab) {
                    let r = ui
                        .add(
                            crate::components::search_field(&mut t.filter, name, width)
                                .margin(inset::BAR),
                        )
                        .accessible_name(name);
                    if settings && self.settings_ui.search_drawn(ui, focused) {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        // TextEdit releases focus on Enter; don't let that same key open a row.
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    }
                }
            }
            HeaderBox::Word
            | HeaderBox::Name
            | HeaderBox::Topic
            | HeaderBox::Reference
            | HeaderBox::Lexicon
            | HeaderBox::Headword => {
                let shown = self.location_text(tab);
                let hint = match kind {
                    HeaderBox::Word => "Strong's number or word",
                    HeaderBox::Name => "Person or place",
                    HeaderBox::Topic => "Topic",
                    HeaderBox::Lexicon => "Strong's number or Lemma",
                    HeaderBox::Headword => "Headword",
                    _ => "Go to passage",
                };
                let Some(t) = self.tabs.get_mut(&tab) else {
                    return;
                };
                let mut text = t.edit.clone().unwrap_or(shown);
                let r = ui
                    .add(
                        egui::TextEdit::singleline(&mut text)
                            .id_salt(("refbox", tab))
                            .hint_text(hint)
                            .desired_width(width)
                            .margin(inset::BAR),
                    )
                    .accessible_name(hint);
                if r.has_focus() {
                    t.edit = Some(text.clone());
                }
                if r.lost_focus() {
                    t.edit = None;
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.session.workspace.user_focused(tab);
                        let out = self
                            .session
                            .run("tab.go", &json!({ "tab": tab.0, "text": text }));
                        if let Some(t) = self.tabs.get_mut(&tab) {
                            t.error = out.err().map(|e| match e {
                                obs_engine::CommandError::Failed { message, .. } => message,
                                e => e.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// The Tab header's ⋯ menu; `narrow`: the header has no room for the Filters button, so
    /// a Resource Tab's Visual filters are here.
    fn tab_menu(&mut self, ui: &mut Ui, tab: TabId, narrow: bool) {
        let resource = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Resource(id)) => Some(id.clone()),
            _ => None,
        };
        let library = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.kind == TabKind::Library);
        let guide = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Guide(g)) if !g.sections().is_empty() => Some(*g),
            _ => None,
        };
        let findable = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.kind.has_find_bar());
        let find_key = key_of(&self.session, "tab.find");
        let close_key = key_of(&self.session, "tab.close");
        crate::components::menu(
            ui,
            icons::image(ui.ctx(), icons::Icon::More, icons::BODY, theme::c().muted),
            |ui| {
                if findable
                    && ui
                        .add(crate::components::button("Find…").shortcut_text(find_key))
                        .clicked()
                {
                    self.session.workspace.user_focused(tab);
                    self.run("tab.find", &json!({ "tab": tab.0 }));
                    ui.close();
                }
                if library && ui.button("Import…").clicked() {
                    self.run("library.import", &json!({}));
                    ui.close();
                }
                if let Some(g) = guide {
                    self.guide_menu(ui, g);
                }
                if narrow && resource.is_some() {
                    self.filters_submenu(ui, tab);
                }
                if let Some(id) = &resource
                    && ui.button("About").clicked()
                {
                    self.run("resource.about", &json!({ "id": id }));
                    ui.close();
                }
                self.favorite_folder_menu(ui, tab);
                self.window_menu(ui, tab);
                if ui
                    .add(crate::components::button("Close Tab").shortcut_text(close_key))
                    .clicked()
                {
                    self.run("tab.close", &json!({ "tab": tab.0 }));
                    ui.close();
                }
                // Its Panel and every Tab in it, as one Recently closed entry (#214).
                if ui.button("Close Panel").clicked() {
                    self.run("panel.close", &json!({ "tab": tab.0 }));
                    ui.close();
                }
            },
        )
        .response
        .accessible_name("Tab menu");
    }

    /// Ask for a Layout name (Save Layout as…, Rename…).
    fn layout_prompt(&mut self, ctx: &egui::Context) {
        let Some((rename, mut text)) = self.prompt.take() else {
            return;
        };
        let title = if rename.is_some() {
            "Rename layout"
        } else {
            "Save layout as"
        };
        let mut done: Option<bool> = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let r = ui
                    .add(
                        egui::TextEdit::singleline(&mut text)
                            .id_salt("layout-name")
                            .hint_text("Layout name")
                            .desired_width(size::FIELD_WIDE),
                    )
                    .accessible_name("Layout name");
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        done = Some(false);
                    }
                });
            });
        match done {
            None => self.prompt = Some((rename, text)),
            Some(false) => {}
            Some(true) if text.trim().is_empty() => self.prompt = Some((rename, text)),
            Some(true) => match rename {
                None => {
                    self.run("layout.save", &json!({ "name": text.trim() }));
                }
                Some(from) => {
                    self.run("layout.rename", &json!({ "name": from, "to": text.trim() }));
                }
            },
        }
    }

    /// Toasts and the About/Credits window, the Note editor when `note_here` and the Move Note
    /// box when `reanchor_here` (their Tab is not in a floating OS window, which draws them).
    fn overlays(&mut self, ui: &Ui, note_here: bool, reanchor_here: bool) {
        let ctx = ui.ctx().clone();
        if note_here {
            self.note_editor_ui(&ctx);
        }
        self.layout_prompt(&ctx);
        self.remove_prompt(&ctx);
        if reanchor_here {
            self.reanchor_prompt(&ctx);
        }
        self.import_chooser(&ctx);
        self.fonts_window(&ctx);
        self.highlight_styles_window(&ctx);
        self.visual_filters_window(&ctx);
        self.copy_window(&ctx);
        self.collection_editor_window(&ctx);
        self.page_tone_hint_ui(&ctx);
        self.update_notice(&ctx);
        self.report_problem_window(&ctx);
        if let Some((msg, t)) = &self.toast {
            if self.now - t < 4.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::unique("toast"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -size::TOAST_RISE))
                    .show(&ctx, |ui| {
                        ui.set_max_width(screen.width() * share::DIALOG);
                        crate::components::popover_frame().show(ui, |ui| {
                            ui.label(RichText::new(msg).color(theme::c().lapis));
                        });
                    });
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            } else {
                self.toast = None;
            }
        }
        if let Some((title, body)) = &self.window {
            let mut open = true;
            egui::Window::new(title.as_str())
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.set_max_width(size::PROSE);
                    ui.label(body.as_str());
                });
            if !open {
                self.window = None;
            }
        }
    }
}

const RESOURCE_TYPES: [ResourceType; 6] = [
    ResourceType::Bible,
    ResourceType::Commentary,
    ResourceType::Lexicon,
    ResourceType::Dictionary,
    ResourceType::CrossReferenceSet,
    ResourceType::PeopleAndPlaces,
];

fn type_name(t: ResourceType) -> &'static str {
    match t {
        ResourceType::Bible => "Bibles",
        ResourceType::Commentary => "Commentaries",
        ResourceType::Lexicon => "Lexicons",
        ResourceType::Dictionary => "Dictionaries",
        ResourceType::CrossReferenceSet => "Cross-references",
        ResourceType::PeopleAndPlaces => "People and places",
    }
}

/// "Standard: Genesis 31:55" beside a verse number whose Standard equivalent differs.
fn verse_number_tooltip(ui: &Ui, doc: &ReadingDoc, native: Verse) {
    let Some(map) = doc.verse_map.as_deref() else {
        return;
    };
    let text = match obs_engine::link::to_standard(Some(map), native) {
        obs_engine::link::ToStandard::Verse(std) if std == native => return,
        obs_engine::link::ToStandard::Verse(std) => {
            format!(
                "Standard numbering: {}",
                format_passage(Passage::verse(std), &Standard, RefStyle::Long)
            )
        }
        obs_engine::link::ToStandard::Unmapped => {
            "No Standard equivalent: Link sets do not follow this verse".to_string()
        }
    };
    egui::Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        ui.scope_id().with("verse-number"),
        egui::PopupAnchor::Pointer,
    )
    .show(|ui| ui.label(text));
}

impl State {
    /// The link chip: the Tab's Link set letter in its colour; click to change it.
    fn link_chip(&mut self, ui: &mut Ui, tab: TabId) {
        let link = self.session.workspace.tab(tab).and_then(|t| t.link);
        let txt = RichText::new(link.map_or_else(|| "–".to_string(), String::from))
            .strong()
            .size(text::SMALL)
            .color(theme::text_on(theme::link_color(link)));
        let resp = ui
            .add(
                crate::components::button(txt)
                    .fill(theme::link_color(link))
                    .stroke(Stroke::NONE)
                    .corner_radius(radius::PILL)
                    .min_size(vec2(size::ROW_COMPACT, size::CONTROL_COMPACT)),
            )
            .on_hover_text(
                "Link set: Tabs with the same letter follow one location. Click to change.",
            )
            .accessible_name(
                link.map_or_else(|| "Not linked".to_string(), |c| format!("Link set {c}")),
            );
        egui::Popup::menu(&resp).show(|ui| self.link_menu(ui, tab));
    }

    /// Float in new window, or Move to main window for a Tab in a floating window (#152).
    fn window_menu(&mut self, ui: &mut Ui, tab: TabId) {
        let ws = &self.session.workspace;
        let window = ws.window_of(tab);
        let alone = window.is_some_and(|w| ws.window_tabs(w).len() == 1);
        let mut run = None;
        if !alone && ui.button("Float in new window").clicked() {
            run = Some("tab.float");
        }
        if window.is_some() && ui.button("Move to main window").clicked() {
            run = Some("tab.dock");
        }
        if let Some(id) = run {
            self.run(id, &json!({ "tab": tab.0 }));
            ui.close();
        }
    }

    fn link_menu(&mut self, ui: &mut Ui, tab: TabId) {
        let current = self.session.workspace.tab(tab).and_then(|t| t.link);
        let choices = std::iter::once(None).chain(obs_engine::LINK_SETS.iter().copied().map(Some));
        for l in choices {
            let label = l.map_or_else(|| "Not linked".to_string(), |c| format!("Link set {c}"));
            if ui.selectable_label(current == l, label).clicked() {
                let set = l.map(String::from).unwrap_or_default();
                self.run("tab.link", &json!({ "tab": tab.0, "set": set }));
                ui.close();
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load(path: &std::path::Path) -> Result<Arc<ReadingDoc>, String> {
    ReadingDoc::open(path)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(target_arch = "wasm32")]
fn load(path: &std::path::Path) -> Result<Arc<ReadingDoc>, String> {
    Err(format!(
        "{}: opening files is not supported here",
        path.display()
    ))
}

impl egui_tiles::Behavior<TabId> for State {
    fn pane_ui(&mut self, ui: &mut Ui, _tile: TileId, tab: &mut TabId) -> UiResponse {
        if self.timed_tab.tab != Some(*tab) {
            self.tab_body(ui, *tab);
            return UiResponse::None;
        }
        let start = std::time::Instant::now();
        self.tab_body(ui, *tab);
        if !self.timed_tab.slowdown.is_zero() {
            std::thread::sleep(self.timed_tab.slowdown);
        }
        let took = self.timed_tab.took.unwrap_or_default() + start.elapsed();
        self.timed_tab.took = Some(took);
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, tab: &TabId) -> egui::WidgetText {
        let focused = self.session.workspace.focused() == Some(*tab);
        let title = self
            .session
            .workspace
            .tab(*tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        crate::components::label_text(title)
            .color(if focused {
                theme::c().ink
            } else {
                theme::c().muted
            })
            .into()
    }

    fn is_tab_closable(&self, _tiles: &Tiles<TabId>, _tile: TileId) -> bool {
        true
    }

    fn on_tab_button(
        &mut self,
        tiles: &mut Tiles<TabId>,
        tile_id: TileId,
        resp: egui::Response,
    ) -> egui::Response {
        if let Some(Tile::Pane(t)) = tiles.get(tile_id) {
            let t = *t;
            if resp.clicked() {
                self.session.workspace.user_focused(t);
            }
            self.tab_button_drawn(t, &resp);
            let linkable = self
                .session
                .workspace
                .tab(t)
                .is_some_and(|tab| tab.kind.has_link_chip());
            resp.context_menu(|ui| {
                if linkable {
                    self.link_menu(ui, t);
                    ui.separator();
                }
                self.window_menu(ui, t);
            });
        }
        resp
    }

    fn top_bar_right_ui(
        &mut self,
        _tiles: &Tiles<TabId>,
        ui: &mut Ui,
        tile_id: TileId,
        _tabs: &egui_tiles::Tabs,
        _scroll: &mut f32,
    ) {
        let panel = self.panel_for_tile(tile_id);
        crate::components::menu(
            ui,
            icons::image(ui.ctx(), icons::Icon::Add, icons::BODY, theme::c().muted),
            |ui| {
                let split = ui.input(|i| i.modifiers.command);
                let mut picked: Option<(&'static str, Value)> = None;
                let bibles: Vec<(String, String)> = self
                    .session
                    .library
                    .entries()
                    .iter()
                    .map(|e| {
                        (
                            e.manifest.resource_id.clone(),
                            format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                        )
                    })
                    .collect();
                for (id, label) in bibles {
                    if ui.button(label).clicked() {
                        picked = Some(("resource.open", json!({ "id": id })));
                    }
                }
                ui.separator();
                for id in [
                    "library.open",
                    "guide.passage_guide",
                    "guide.exegetical_guide",
                    "guide.word_study",
                    "tool.search",
                    "tool.text_comparison",
                    "tool.notes",
                    "tool.word_info",
                    "tool.cross_references",
                    "tool.atlas",
                    "tool.timeline",
                    "tool.favorites",
                    "tool.history",
                ] {
                    let label = self.session.commands().get(id).map_or(id, |c| c.label);
                    if ui.button(label).clicked() {
                        picked = Some((id, json!({})));
                    }
                }
                ui.label(crate::components::hint_text(
                    "Ctrl/Cmd-click opens a new split",
                ));
                if let Some((id, mut params)) = picked {
                    if let Some(obj) = params.as_object_mut() {
                        obj.insert("split".into(), json!(split));
                        if let Some(p) = panel {
                            obj.insert("panel".into(), json!(p.0));
                        }
                    }
                    self.run(id, &params);
                    ui.close();
                }
            },
        )
        .response
        .accessible_name("Open a Tab in this Panel");
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        size::TAB_BAR
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        space::S
    }

    fn tab_title_spacing(&self, _visuals: &egui::Visuals) -> f32 {
        space::L
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        theme::c().shell
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<TabId>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        if state.active {
            theme::c().surface
        } else {
            Color32::TRANSPARENT
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<TabId>,
        _tile_id: TileId,
        _state: &egui_tiles::TabState,
    ) -> Stroke {
        Stroke::NONE
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> Stroke {
        crate::components::rule_stroke()
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        match state {
            egui_tiles::ResizeState::Idle => Stroke::new(stroke::HAIR, theme::c().shell),
            _ => Stroke::new(stroke::THICK, theme::c().lapis),
        }
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

/// Scroll a widget the keyboard has just moved to into view (#182): egui's Tab moves to
/// widgets scrolled out of sight without scrolling, which would leave the focus ring out of
/// sight too. It asks as a screen reader does (AccessKit's Scroll into view), which the
/// widget's own scroll area answers as the widget is drawn. Only when the keyboard has just
/// moved, so scrolling away from it with the wheel stays put; and not for a widget over half
/// the screen tall (a reading pane), which can never be wholly in view.
fn scroll_focus_into_view(ctx: &egui::Context) {
    // Each OS window has its own focus; the one last scrolled to is kept per window.
    let last_id = egui::Id::unique("focus-scrolled-into-view").with(ctx.viewport_id());
    let focused = ctx.memory(egui::Memory::focused);
    let last = ctx.data_mut(|d| {
        let last = d.get_temp::<egui::Id>(last_id);
        d.insert_temp(last_id, focused.unwrap_or(egui::Id::NULL));
        last
    });
    let Some(id) = focused.filter(|&id| last != Some(id)) else {
        return;
    };
    let Some(r) = ctx.read_response(id) else {
        return;
    };
    if r.interact_rect == r.rect || r.rect.height() > ctx.content_rect().height() / 2.0 {
        return;
    }
    let request = egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::ScrollIntoView,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: id.accesskit_id(),
        data: None,
    };
    ctx.input_mut(|i| i.events.push(egui::Event::AccessKitActionRequest(request)));
}

/// The ring round the widget that has the keyboard (#182). egui's own widgets show focus only
/// by a change of colour, and those drawn with the painter not at all; one ring for all of them
/// shows where Tab went. Text fields keep their own border and caret.
fn focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(egui::Memory::focused) else {
        return;
    };
    if egui::text_edit::TextEditState::load(ctx, id).is_some() {
        return;
    }
    let Some(r) = ctx.read_response(id) else {
        return;
    };
    // Only what is in view: a widget scrolled partly away is ringed where it shows.
    let rect = r.interact_rect;
    if !rect.is_positive() {
        return;
    }
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(FOCUS_RING));
    ctx.layer_painter(layer).rect_stroke(
        rect.expand(space::PX),
        radius::S,
        Stroke::new(FOCUS_RING_WIDTH, theme::c().lapis),
        egui::StrokeKind::Outside,
    );
}

/// A link that moves to the next row whole when it does not fit in a wrapping row: a link
/// broken across two rows is two widgets in egui, and screen readers get no name for the
/// second (#182).
pub(crate) fn unbroken_link(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let wrap = ui.style().wrap_mode;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    let r = ui.link(text);
    ui.style_mut().wrap_mode = wrap;
    r
}

/// The focus ring's layer, and its width in points.
pub(crate) const FOCUS_RING: &str = "focus-ring";
pub(crate) const FOCUS_RING_WIDTH: f32 = stroke::THICK;

/// Call before drawing a window that Esc closes; the closure, called after with the layer the
/// window was drawn on, says whether Esc closes it now (#182). Esc closes one window: the one
/// with the keyboard, or with the keyboard in the Panels or nowhere, the one last brought to
/// the front; none while the keyboard is in another overlay (the Note editor, a toolbar pane)
/// or a menu or drop-down was open when the frame began (Esc closes that first).
fn escape_closes_window(ctx: &egui::Context) -> impl FnOnce(Option<egui::LayerId>) -> bool + '_ {
    let popup = egui::Popup::is_any_open(ctx);
    move |layer| {
        let Some(layer) = layer else {
            return false;
        };
        let (front, keyboard) = escape_windows_note(ctx, layer);
        if popup || !ctx.input(|i| i.key_pressed(egui::Key::Escape) && i.modifiers.is_none()) {
            return false;
        }
        let ours = match keyboard {
            // The keyboard is in a window or overlay of its own: Esc is for that one.
            Some(l) if l.order != egui::Order::Background => l == layer,
            _ => front == layer,
        };
        ours && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    }
}

/// What [`escape_closes_window`] keeps between frames.
#[derive(Clone, Default)]
struct EscapeWindows {
    /// The open windows Esc closes, back to front, with the pass each was last drawn in.
    windows: Vec<(egui::LayerId, u64)>,
    /// The layer the keyboard was in as the last pass ended (Esc takes the keyboard from its
    /// widget before anything is drawn), and as this pass's windows were drawn.
    keyboard_before: Option<egui::LayerId>,
    keyboard_now: (u64, Option<egui::LayerId>),
}

/// Note `layer` drawn this pass (to the front when it has just opened or is pressed on, as
/// egui raises a window); return the front window and where the keyboard was before this pass.
fn escape_windows_note(
    ctx: &egui::Context,
    layer: egui::LayerId,
) -> (egui::LayerId, Option<egui::LayerId>) {
    let pass = ctx.cumulative_pass_nr();
    let pressed_on = (ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos())))
        .flatten()
        .is_some_and(|p| ctx.layer_id_at(p) == Some(layer));
    let keyboard = (ctx.memory(|m| m.focused()))
        .and_then(|id| ctx.read_response(id))
        .map(|r| r.layer_id);
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<EscapeWindows>(egui::Id::unique("escape-windows"));
        if s.keyboard_now.0 != pass {
            s.keyboard_before = s.keyboard_now.1;
        }
        s.keyboard_now = (pass, keyboard);
        // Drawn in neither this pass nor the last: closed.
        s.windows.retain(|&(_, seen)| seen + 1 >= pass);
        match s.windows.iter().position(|&(l, _)| l == layer) {
            Some(i) if !pressed_on => s.windows[i].1 = pass,
            _ => {
                s.windows.retain(|&(l, _)| l != layer);
                s.windows.push((layer, pass));
            }
        }
        let front = s.windows.last().map_or(layer, |&(l, _)| l);
        (front, s.keyboard_before)
    })
}

impl State {
    /// The Panel a Tabs container shows.
    fn panel_for_tile(&self, tile: TileId) -> Option<PanelId> {
        self.panels.get(&tile).copied()
    }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
