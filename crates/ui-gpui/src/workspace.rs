//! The Workspace on GPUI (#385): the engine's Panels and Tabs drawn by `gpui-base`'s dock
//! (`DockArea`), as `obs-ui-egui` draws them with `egui_tiles`.
//!
//! The engine's [`obs_engine::Workspace`] decides what opens where ([`Placement`]) and keeps
//! the Tabs; the dock is what the user drags. The two are kept in step the way the egui Shell
//! does it:
//!
//! - the engine's Effects ([`Effect::Opened`], `Focus`, `Activate`, `Closed`, `Rebuild`,
//!   `ResetLayout`) are applied to the dock ([`crate::Shell::apply_effects`]);
//! - after every change of the dock (a drag, a split, a ×), the Panels it shows are told to
//!   the engine (`Workspace::sync_panels`, which closes Tabs no Panel shows: Recently closed,
//!   #214) and its arrangement is recorded (`Workspace::arrangement`, saved in Layouts and the
//!   Last session) ([`crate::Shell::record_layout`]).
//!
//! Each engine Tab is one dock panel ([`TabView`]): the Tab header strip (link chip, ‹ ›,
//! the reference or Key box, title, Aa, ⋯) over the Tab's content ([`tab_content`], the hook
//! the reading pane, #380, and the Tools plug into). Each dock tab group is one engine Panel;
//! [`DockMap`] remembers which.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::dock::{
    DockArea, DockAreaRenderer, DockLayout, DockPlacement, InsertTarget, NodeId, PaneNode, PaneRef,
    Panel, PanelEvent, PanelId as DockPanelId, PanelView, TabGroupContext, TabGroupRenderer,
};
use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::base::{Button, Placement as DockSide};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Axis, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Pixels,
    Render, Role, SharedString, Stateful, WeakEntity, Window, div, px,
};
use obs_engine::persist::LayoutNode;
use obs_engine::{HeaderBox, PanelId, Placement, Side, TabId, TabKind};
use obs_scripture::{Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use crate::Shell;
use crate::components;
use crate::icons::Icon;
use crate::theme::{self, Look};
use crate::tokens::{self, rgba, size, space, text};

/// The dock id written into its own state (not used for persistence: the engine's
/// [`LayoutNode`] is what Layouts and the Last session keep).
pub const DOCK_ID: &str = "workspace";

/// Which engine Panel each dock tab group is, and which engine Tab each dock panel is.
#[derive(Default)]
pub struct DockMap {
    /// Dock tab group → engine Panel.
    pub groups: BTreeMap<NodeId, PanelId>,
    /// Engine Tab → its view.
    pub views: HashMap<TabId, Entity<TabView>>,
    /// Dock panel → engine Tab.
    pub tabs: HashMap<DockPanelId, TabId>,
}

impl DockMap {
    pub fn clear(&mut self) {
        self.groups.clear();
        self.views.clear();
        self.tabs.clear();
    }

    /// The dock tab group that is engine Panel `panel`.
    pub fn group_of(&self, panel: PanelId) -> Option<NodeId> {
        self.groups
            .iter()
            .find(|(_, p)| **p == panel)
            .map(|(n, _)| *n)
    }

    pub fn dock_panel(&self, tab: TabId) -> Option<DockPanelId> {
        self.views
            .get(&tab)
            .map(|v| DockPanelId::from(v.entity_id()))
    }
}

/// The dock side for the engine's [`Side`].
pub fn dock_side(side: Side) -> DockSide {
    match side {
        Side::Right => DockSide::Right,
        Side::Left => DockSide::Left,
        Side::Below => DockSide::Bottom,
        Side::Above => DockSide::Top,
    }
}

/// The node holding `id` directly, below `root`.
pub fn parent_of(root: &PaneNode, id: NodeId) -> Option<NodeId> {
    if let PaneRef::Split { children, .. } = root.kind() {
        for c in children {
            if c.id() == id {
                return Some(root.id());
            }
            if let Some(p) = parent_of(c, id) {
                return Some(p);
            }
        }
    }
    None
}

/// Every tab group below `node`, in order, with its panels and active index.
pub fn groups_below(node: &PaneNode) -> Vec<(NodeId, Vec<DockPanelId>, usize)> {
    let mut out = Vec::new();
    node.walk(&mut |n| {
        if let PaneRef::Tabs { panels, active_ix } = n.kind() {
            out.push((n.id(), panels.to_vec(), active_ix));
        }
    });
    out
}

/// The dock below `node` as a saved arrangement ([`LayoutNode`]); Tabs the map does not know
/// are left out, and so are groups and splits left empty.
pub fn layout_node(node: &PaneNode, map: &DockMap) -> Option<LayoutNode> {
    match node.kind() {
        PaneRef::Tabs { panels, active_ix } => {
            let tabs: Vec<u64> = panels
                .iter()
                .filter_map(|p| map.tabs.get(p).map(|t| t.0))
                .collect();
            if tabs.is_empty() {
                return None;
            }
            let active = panels
                .get(active_ix)
                .and_then(|p| map.tabs.get(p))
                .map(|t| t.0);
            let panel = map.groups.get(&node.id()).map_or(0, |p| p.0);
            Some(LayoutNode::Panel {
                panel,
                tabs,
                active,
            })
        }
        PaneRef::Split {
            axis,
            children,
            sizes,
        } => {
            let children: Vec<(LayoutNode, f32)> = children
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let share = sizes
                        .get(i)
                        .copied()
                        .flatten()
                        .map_or(1.0, |s: Pixels| f32::from(s).max(1.0));
                    Some((layout_node(c, map)?, share))
                })
                .collect();
            match children.len() {
                0 => None,
                1 => children.into_iter().next().map(|(n, _)| n),
                _ => Some(match axis {
                    Axis::Horizontal => LayoutNode::Row { children },
                    Axis::Vertical => LayoutNode::Column { children },
                }),
            }
        }
    }
}

/// A saved arrangement as a dock layout, with the views of `tab_view`; Tabs it has no view
/// for are left out. Returns the layout and, per tab group in build order, the engine Panel it
/// is (0: a new one).
pub fn dock_layout(
    node: &LayoutNode,
    tab_view: &mut dyn FnMut(TabId) -> Option<Arc<dyn PanelView>>,
    cx: &App,
) -> Option<DockLayout> {
    match node {
        LayoutNode::Panel { tabs, active, .. } => {
            let mut layout = DockLayout::tabs();
            let mut shown = Vec::new();
            for t in tabs {
                if let Some(view) = tab_view(TabId(*t)) {
                    layout = layout.panel_view(view, cx);
                    shown.push(*t);
                }
            }
            if shown.is_empty() {
                return None;
            }
            if let Some(i) = active.and_then(|a| shown.iter().position(|t| *t == a)) {
                layout = layout.active_index(i);
            }
            Some(layout)
        }
        LayoutNode::Row { children } | LayoutNode::Column { children } => {
            let built: Vec<DockLayout> = children
                .iter()
                .filter_map(|(c, _)| dock_layout(c, tab_view, cx))
                .collect();
            if built.len() <= 1 {
                return built.into_iter().next();
            }
            let mut layout = if matches!(node, LayoutNode::Row { .. }) {
                DockLayout::h_split()
            } else {
                DockLayout::v_split()
            };
            for b in built {
                layout = layout.child(b, None);
            }
            Some(layout)
        }
    }
}

/// The text the Tab header's box shows: the Tab's location in its own numbering (a Verse or
/// a Passage), or its entry Key, as the egui Shell's `location_text` (simplified: Word Study,
/// Factbook and Topic choosers come with their Tools).
pub fn location_text(shell: &Shell, tab: TabId) -> String {
    let Some(t) = shell.session().workspace.tab(tab) else {
        return String::new();
    };
    if matches!(
        t.header_box(),
        HeaderBox::Lexicon
            | HeaderBox::Headword
            | HeaderBox::Word
            | HeaderBox::Topic
            | HeaderBox::Name
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
    if let Some(p) = t.shown_passage().filter(|p| !p.is_single_verse()) {
        return format_passage(p, &Standard, RefStyle::Long);
    }
    let p = Passage::verse(v);
    match &t.versification {
        Some(own) => format_passage(p, own.as_ref(), RefStyle::Long),
        None => format_passage(p, &Standard, RefStyle::Long),
    }
}

/// The accessible name of a Tab's header box, by what it takes.
pub fn box_label(kind: HeaderBox, title: &str) -> String {
    match kind {
        HeaderBox::Reference => format!("Go to a reference in {title}"),
        HeaderBox::Lexicon | HeaderBox::Headword => format!("Go to an entry in {title}"),
        HeaderBox::Word => format!("Study a word in {title}"),
        HeaderBox::Name => format!("Look up a name in {title}"),
        HeaderBox::Topic => format!("Look up a topic in {title}"),
        _ => format!("Filter {title}"),
    }
}

/// The content of a Tab under its header strip: the hook the reading pane (#380) and the
/// Tools (#389–#397) plug into, one `match` arm each. Until then every Tab kind shows a
/// placeholder naming what it will show.
pub fn tab_content(shell: &Shell, tab: TabId, look: &Look) -> AnyElement {
    let Some(t) = shell.session().workspace.tab(tab) else {
        return div().into_any_element();
    };
    let what = match &t.kind {
        TabKind::Resource(id) => format!("The reading pane for {id} comes with #380."),
        other => format!(
            "{} comes with its Tool's ticket.",
            other_title(other, &t.title)
        ),
    };
    components::page(look)
        .id(("tab-content", tab.0))
        .flex_1()
        .min_h(px(0.0))
        .child(components::hint(what, look))
        .into_any_element()
}

fn other_title(_kind: &TabKind, title: &str) -> String {
    title.to_string()
}

/// What a Tab header strip shows, read from the Session before drawing.
pub struct HeaderState {
    pub title: String,
    pub link: Option<char>,
    pub has_chip: bool,
    pub header_box: HeaderBox,
    pub can_back: bool,
    pub can_forward: bool,
    pub location: String,
}

impl HeaderState {
    pub fn of(shell: &Shell, tab: TabId) -> Option<Self> {
        let t = shell.session().workspace.tab(tab)?;
        Some(Self {
            title: t.title.clone(),
            link: t.link,
            has_chip: t.kind.has_link_chip(),
            header_box: t.header_box(),
            can_back: t.history.can_go_back() || t.entry_history.can_go_back(),
            can_forward: t.history.can_go_forward() || t.entry_history.can_go_forward(),
            location: location_text(shell, tab),
        })
    }
}

/// One engine Tab in the dock: its header strip and its content.
pub struct TabView {
    shell: WeakEntity<Shell>,
    pub tab: TabId,
    focus: FocusHandle,
    /// The header's reference or Key box.
    reference: Entity<InputState>,
    /// The ⋯ menu is open.
    menu_open: bool,
}

impl TabView {
    pub fn new(
        shell: WeakEntity<Shell>,
        tab: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let reference = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&reference, window, |this, state, event, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = state.read(cx).value().to_string();
                let tab = this.tab;
                let shell = this.shell.clone();
                cx.defer(move |cx| {
                    if let Some(shell) = shell.upgrade() {
                        shell.update(cx, |s, cx| {
                            s.run_in(cx, "tab.go", &json!({ "tab": tab.0, "text": text }));
                        });
                    }
                });
            }
        })
        .detach();
        Self {
            shell,
            tab,
            focus: cx.focus_handle(),
            reference,
            menu_open: false,
        }
    }

    fn run(&self, cx: &mut App, id: &'static str, params: serde_json::Value) {
        let shell = self.shell.clone();
        cx.defer(move |cx| {
            if let Some(shell) = shell.upgrade() {
                shell.update(cx, |s, cx| {
                    s.run_in(cx, id, &params);
                });
            }
        });
    }

    fn header(
        &mut self,
        state: HeaderState,
        look: &Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let ch = look.chrome;
        let tab = self.tab;
        let HeaderState {
            title,
            link,
            has_chip,
            header_box,
            can_back,
            can_forward,
            location,
        } = state;
        // The box shows the location while it is not being typed in.
        let focused = self.reference.read(cx).focus_handle(cx).is_focused(window);
        if !focused && self.reference.read(cx).value().as_ref() != location {
            self.reference
                .update(cx, |s, cx| s.set_value(location, window, cx));
        }
        let chip_label: SharedString = match link {
            Some(c) => format!("Link set {c} for {title}").into(),
            None => format!("Not linked: {title}").into(),
        };
        let chip_text: SharedString = link.map_or("–".to_string(), |c| c.to_string()).into();
        div()
            .id(("tab-header", tab.0))
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .gap(px(look.pt(space::XS)))
            .px(px(look.pt(space::S)))
            .py(px(look.pt(space::XS)))
            .bg(rgba(ch.surface))
            .border_b_1()
            .border_color(rgba(ch.rule))
            .when(has_chip, |row| {
                row.child(
                    components::button(("link-chip", tab.0), chip_text, look)
                        .accessibility_label(chip_label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // ‹link chip›: the next Link set, then none (the egui chip's menu
                            // comes with #386).
                            let next = match link {
                                None => "A",
                                Some('A') => "B",
                                Some('B') => "C",
                                _ => "",
                            };
                            this.run(cx, "tab.link", json!({ "tab": tab.0, "set": next }));
                        })),
                )
            })
            .child(
                components::icon_button(
                    ("back", tab.0),
                    Icon::Back,
                    format!("Back in {title}"),
                    look,
                )
                .when(!can_back, |b| b.opacity(0.4))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.run(cx, "tab.back", json!({ "tab": tab.0 }));
                })),
            )
            .child(
                components::icon_button(
                    ("forward", tab.0),
                    Icon::Forward,
                    format!("Forward in {title}"),
                    look,
                )
                .when(!can_forward, |b| b.opacity(0.4))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.run(cx, "tab.forward", json!({ "tab": tab.0 }));
                })),
            )
            .child(components::text_field(
                ("reference", tab.0),
                &self.reference,
                box_label(header_box, &title),
                look,
                window,
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(rgba(ch.muted))
                    .text_size(px(look.pt(text::SMALL)))
                    .child(SharedString::from(title.clone())),
            )
            .child(
                components::button(("aa", tab.0), "Aa", look)
                    .accessibility_label(format!("Text size and layout for {title}")),
            )
            .child(
                components::icon_button(
                    ("more", tab.0),
                    Icon::More,
                    format!("More for {title}"),
                    look,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu_open = !this.menu_open;
                    cx.notify();
                })),
            )
    }

    fn more_menu(&self, look: &Look, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.menu_open {
            return None;
        }
        let tab = self.tab;
        Some(
            components::menu("Tab menu", look)
                .absolute()
                .top(px(look.pt(size::CONTROL) + look.pt(space::M)))
                .right(px(look.pt(space::S)))
                .child(
                    components::menu_item(("close-tab", tab.0), "Close Tab", None, look).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.menu_open = false;
                            this.run(cx, "tab.close", json!({ "tab": tab.0 }));
                        }),
                    ),
                )
                .child(
                    components::menu_item(("reopen", tab.0), "Reopen closed Tab", None, look)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu_open = false;
                            this.run(cx, "tab.reopen_closed", json!({}));
                        })),
                )
                .into_any_element(),
        )
    }
}

impl EventEmitter<PanelEvent> for TabView {}

impl Focusable for TabView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Panel for TabView {
    fn panel_name(&self) -> &'static str {
        "tab"
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Render for TabView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = theme::look(cx);
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        let Some(state) = HeaderState::of(shell.read(cx), self.tab) else {
            return div().into_any_element();
        };
        let header = self.header(state, &look, window, cx);
        let content = tab_content(shell.read(cx), self.tab, &look);
        div()
            .id(("tab-view", self.tab.0))
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(content)
            .children(self.more_menu(&look, cx))
            .into_any_element()
    }
}

/// The dock's appearance: the Workspace's tab bars and Panel frames in the current Look.
pub struct WorkspaceSkin {
    pub shell: WeakEntity<Shell>,
}

impl DockAreaRenderer for WorkspaceSkin {
    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(TabStrip {
            shell: self.shell.clone(),
        })
    }
}

/// A Panel's Tab strip: one tab per Tab (its title; drag it to another Panel, or to a Panel's
/// edge to split), its × and the Panel's +.
pub struct TabStrip {
    shell: WeakEntity<Shell>,
}

impl TabStrip {
    fn tab_of(&self, panel: &Arc<dyn PanelView>, cx: &App) -> Option<TabId> {
        panel
            .as_any()
            .downcast_ref::<Entity<TabView>>()
            .map(|v| v.read(cx).tab)
    }
}

impl TabGroupRenderer for TabStrip {
    fn frame(&self, group: &TabGroupContext, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let look = theme::look(cx);
        let ch = look.chrome;
        div()
            .id(("panel", group.node().as_u64()))
            .m(px(look.pt(space::XXS)))
            .border_1()
            .border_color(rgba(ch.rule))
            .rounded(px(look.pt(tokens::radius::M)))
            .overflow_hidden()
            .bg(rgba(ch.surface))
    }

    fn render_tab_bar(&self, group: &TabGroupContext, _: &mut Window, cx: &mut App) -> AnyElement {
        let look = theme::look(cx);
        let ch = look.chrome;
        let node = group.node();
        let titles: Vec<(usize, TabId, String)> = group
            .panels()
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let tab = self.tab_of(p, cx)?;
                let title = self
                    .shell
                    .upgrade()
                    .and_then(|s| {
                        s.read(cx)
                            .session()
                            .workspace
                            .tab(tab)
                            .map(|t| t.title.clone())
                    })
                    .unwrap_or_default();
                Some((i, tab, title))
            })
            .collect();
        let active = group.active_ix();
        let draggable = group.is_draggable();
        let mut bar = div()
            .id(("tab-bar", node.as_u64()))
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(look.pt(size::ROW)))
            .bg(rgba(ch.shell))
            .border_b_1()
            .border_color(rgba(ch.rule));
        for (ix, tab, title) in titles {
            let selected = ix == active;
            let label: SharedString = title.clone().into();
            let select = group.clone();
            let close = group.clone();
            let panel_id = group.panels().get(ix).map(|p| p.panel_id(cx));
            let mut tab_el = Button::new(("tab", tab.0))
                .role(Role::Tab)
                .accessibility_label(label.clone())
                .px(px(look.pt(space::M)))
                .h_full()
                .flex()
                .items_center()
                .text_size(px(look.pt(text::BODY)))
                .text_color(rgba(if selected { ch.ink } else { ch.muted }))
                .when(selected, |b| b.bg(rgba(ch.surface)))
                .child(label.clone())
                .on_click({
                    let shell = self.shell.clone();
                    move |_, window, cx| {
                        select.select_tab(ix, window, cx);
                        // A click on a tab focuses its Tab, as in egui (a Tab shown by the
                        // dock itself, a rebuild or a closed neighbour, does not).
                        let shell = shell.clone();
                        cx.defer(move |cx| {
                            if let Some(shell) = shell.upgrade() {
                                shell.update(cx, |s, cx| s.tab_shown(tab, cx));
                            }
                        });
                    }
                });
            if draggable && let Some(drag) = group.drag_panel(ix, cx) {
                tab_el = tab_el.on_drag(drag, |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                });
            }
            let close_btn = components::icon_button(
                ("close", tab.0),
                Icon::Close,
                format!("Close {title}"),
                &look,
            )
            .on_click(move |_, window, cx| {
                if let Some(p) = panel_id {
                    close.close(p, window, cx);
                }
            });
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h_full()
                    .border_r_1()
                    .border_color(rgba(ch.rule))
                    .child(tab_el)
                    .child(close_btn),
            );
        }
        let shell = self.shell.clone();
        bar.child(div().flex_1())
            .child(
                components::icon_button(
                    ("plus", node.as_u64()),
                    Icon::Add,
                    "Open a Resource or Tool in this Panel",
                    &look,
                )
                .on_click(move |_, _, cx| {
                    if let Some(shell) = shell.upgrade() {
                        shell.update(cx, |s, cx| {
                            s.plus_menu = Some(node);
                            cx.notify();
                        });
                    }
                }),
            )
            .into_any_element()
    }
}

/// The Workspace's side of the Shell: the dock kept in step with the engine.
impl Shell {
    /// Run a command from a control in the Workspace, then redraw (its Effects are applied
    /// by the next frame, [`Self::apply_effects`]).
    pub fn run_in(
        &mut self,
        cx: &mut Context<Self>,
        id: &str,
        params: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let out = self.run(id, params);
        cx.notify();
        out
    }

    /// The Workspace at launch, as the egui Shell's `restore_at_launch`: the Last session,
    /// else the default arrangement, with Home on top when there was none or when "Open Home
    /// at launch" is on (#178). The dock is built from it by the first frame.
    pub fn restore_at_launch(&mut self) {
        let restored = self.session.restore_last_session();
        if !restored {
            self.reset_engine_layout();
            self.session
                .workspace
                .push_effect(obs_engine::Effect::Rebuild);
        }
        if !restored || self.session.home_opens_at_launch() {
            self.run("home.open", &json!({}));
        }
    }

    /// Make the dock, once: it needs the window.
    pub(crate) fn ensure_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock.is_some() {
            return;
        }
        let shell = cx.weak_entity();
        let dock = cx.new(|cx| {
            DockArea::new(DOCK_ID, None, window, cx).with_renderer(Rc::new(WorkspaceSkin { shell }))
        });
        cx.subscribe(&dock, |this, _, event, cx| {
            if let gpui_kit::base::dock::DockEvent::LayoutChanged = event {
                this.record_layout(cx);
                cx.notify();
            }
        })
        .detach();
        self.dock = Some(dock);
        // Tabs the Session holds already (a restored Last session) are built into the dock.
        if self.session.workspace.tabs().next().is_some() {
            self.session
                .workspace
                .push_effect(obs_engine::Effect::Rebuild);
        }
    }

    /// Apply the engine's Effects to the dock (the egui Shell's `apply_effects`), then tell the
    /// engine what the dock shows.
    pub(crate) fn apply_effects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use obs_engine::Effect;
        if self.dock.is_none() {
            return;
        }
        let mut changed = false;
        loop {
            let effects = self.session.workspace.take_effects();
            if effects.is_empty() {
                break;
            }
            changed = true;
            for e in effects {
                match e {
                    Effect::Opened { tab, placement } => {
                        self.insert_tab(tab, placement, window, cx);
                    }
                    Effect::Focus(tab) | Effect::Activate(tab) | Effect::KeyboardToTab(tab) => {
                        self.show_tab(tab, window, cx);
                    }
                    Effect::Closed(tab) => self.remove_tab_view(tab, window, cx),
                    Effect::Rebuild => self.rebuild(window, cx),
                    Effect::ResetLayout => {
                        self.reset_engine_layout();
                        self.rebuild(window, cx);
                    }
                    Effect::PromptLayoutName { rename } => {
                        let text = rename.clone().unwrap_or_default();
                        let state = cx.new(|cx| InputState::new(window, cx));
                        state.update(cx, |s, cx| s.set_value(text, window, cx));
                        self.layout_prompt = Some((rename, state));
                    }
                    Effect::Message(m) => self.toast = Some(m),
                    // Moving views, Tools' windows and Floating windows come with their
                    // tickets (#380–#397, #388).
                    _ => {}
                }
            }
        }
        if changed {
            self.record_layout(cx);
            self.session.record_history();
        }
    }

    /// Put a Tab's view into the dock where the engine placed it: into Panel `p`, or as a new
    /// Panel on its side of `beside` (or of the whole Workspace), `levels` rows and columns
    /// out (the egui Shell's `insert_pane` and `attach_beside`).
    fn insert_tab(
        &mut self,
        tab: TabId,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dock) = self.dock.clone() else {
            return;
        };
        if self.map.views.contains_key(&tab) {
            self.show_tab(tab, window, cx);
            return;
        }
        let shell = cx.weak_entity();
        let view = cx.new(|cx| TabView::new(shell, tab, window, cx));
        let id = DockPanelId::from(view.entity_id());
        self.map.views.insert(tab, view.clone());
        self.map.tabs.insert(id, tab);
        let (target, new_panel) = {
            let d = dock.read(cx);
            let tree = d.layout(DockPlacement::Center);
            let empty = tree.is_none_or(|t| t.panels().next().is_none());
            match placement {
                Placement::Panel(p) => (
                    self.map.group_of(p).map(|node| InsertTarget::Tabs {
                        node,
                        ix: None,
                        activate: true,
                    }),
                    Some(p),
                ),
                Placement::NewPanel {
                    panel,
                    beside,
                    side,
                    levels,
                } if !empty => {
                    let root = tree.map(|t| t.root());
                    let mut anchor = beside
                        .and_then(|b| self.map.group_of(b))
                        .or_else(|| root.map(|r| r.id()));
                    if let (Some(root), Some(a)) = (root, anchor.as_mut()) {
                        for _ in 0..levels {
                            match parent_of(root, *a) {
                                Some(p) => *a = p,
                                None => break,
                            }
                        }
                    }
                    (
                        anchor.map(|node| InsertTarget::Split {
                            node,
                            placement: dock_side(side),
                            size: None,
                        }),
                        Some(panel),
                    )
                }
                Placement::NewPanel { panel, .. } => (None, Some(panel)),
            }
        };
        dock.update(cx, |d, cx| {
            d.add_panel(view.clone(), DockPlacement::Center, None, window, cx);
            if let Some(target) = target {
                d.move_panel(id, target, window, cx);
            }
            d.select_panel(id, window, cx);
        });
        // The group the Tab landed in is the engine's Panel when it is new.
        if let Some(panel) = new_panel
            && let Some(node) = dock
                .read(cx)
                .layout(DockPlacement::Center)
                .and_then(|t| t.find_panel_node(id))
        {
            self.map.groups.entry(node).or_insert(panel);
        }
    }

    /// Show a Tab in its group.
    fn show_tab(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(dock), Some(id)) = (self.dock.clone(), self.map.dock_panel(tab)) {
            dock.update(cx, |d, cx| d.select_panel(id, window, cx));
        }
    }

    /// Take a closed Tab's view out of the dock.
    fn remove_tab_view(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.map.views.remove(&tab) else {
            return;
        };
        self.map.tabs.retain(|_, t| *t != tab);
        if let Some(dock) = self.dock.clone() {
            dock.update(cx, |d, cx| d.remove_panel(view, window, cx));
        }
    }

    /// A Tab was shown in its group by the user (a click on its tab): it is the focused Tab.
    pub(crate) fn tab_shown(&mut self, tab: TabId, cx: &mut Context<Self>) {
        if self.session.workspace.focused() != Some(tab) && self.map.views.contains_key(&tab) {
            self.session.workspace.user_focused(tab);
            cx.notify();
        }
    }

    /// Rebuild the dock from the engine's Workspace (a Layout or the Last session restored).
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dock) = self.dock.clone() else {
            return;
        };
        self.map.clear();
        self.shown.clear();
        let ids: Vec<TabId> = self.session.workspace.tabs().map(|t| t.id).collect();
        let shell = cx.weak_entity();
        for t in ids {
            let view = cx.new(|cx| TabView::new(shell.clone(), t, window, cx));
            self.map.tabs.insert(DockPanelId::from(view.entity_id()), t);
            self.map.views.insert(t, view);
        }
        let arrangement = self.session.workspace.arrangement.clone().or_else(|| {
            let children: Vec<(LayoutNode, f32)> = self
                .session
                .workspace
                .panels()
                .map(|(p, tabs)| {
                    (
                        LayoutNode::Panel {
                            panel: p.0,
                            tabs: tabs.iter().map(|t| t.0).collect(),
                            active: self.session.workspace.active_in(p).map(|t| t.0),
                        },
                        1.0,
                    )
                })
                .collect();
            (!children.is_empty()).then_some(LayoutNode::Row { children })
        });
        let views = &self.map.views;
        let layout = arrangement.and_then(|a| {
            dock_layout(
                &a,
                &mut |t| {
                    views
                        .get(&t)
                        .map(|v| Arc::new(v.clone()) as Arc<dyn PanelView>)
                },
                cx,
            )
        });
        dock.update(cx, |d, cx| {
            d.set_center(layout.unwrap_or_else(DockLayout::h_split), window, cx);
        });
        // Each group is the engine Panel its Tabs are in.
        let groups = dock
            .read(cx)
            .layout(DockPlacement::Center)
            .map(|t| groups_below(t.root()))
            .unwrap_or_default();
        for (node, panels, _) in groups {
            let engine = panels
                .first()
                .and_then(|p| self.map.tabs.get(p))
                .and_then(|t| self.session.workspace.panel_of(*t));
            if let Some(p) = engine {
                self.map.groups.insert(node, p);
            }
        }
        if let Some(f) = self.session.workspace.focused() {
            self.show_tab(f, window, cx);
        }
    }

    /// The default arrangement (the egui Shell's `reset_layout`), in the engine: the ★ Bible
    /// in one Panel, in Link set A; the Library and Word Info in a Panel to its right; the
    /// Bible focused. The dock is rebuilt from it ([`obs_engine::Effect::Rebuild`]).
    fn reset_engine_layout(&mut self) {
        self.session.workspace.clear();
        let _ = self.session.workspace.take_effects();
        let first = self
            .session
            .library
            .preferred(obs_model::ResourceType::Bible)
            .map(|e| e.manifest.resource_id.clone());
        let mut bible = None;
        if let Some(id) = first
            && let Some(t) = self
                .run("resource.open", &json!({ "id": id }))
                .and_then(|v| v["tab"].as_u64())
        {
            self.run("tab.link", &json!({ "tab": t, "set": "A" }));
            bible = Some(TabId(t));
        }
        self.run("library.open", &json!({}));
        self.run("tool.word_info", &json!({}));
        if let Some(t) = bible {
            self.session.workspace.focus(t);
        }
    }

    /// Tell the engine what the dock shows (after drags, splits and × closes): each Panel's
    /// Tabs (a Tab no Panel shows is closed: Recently closed, #214) and the arrangement.
    pub(crate) fn record_layout(&mut self, cx: &mut Context<Self>) {
        let Some(dock) = self.dock.clone() else {
            return;
        };
        let d = dock.read(cx);
        let Some(tree) = d.layout(DockPlacement::Center) else {
            return;
        };
        let groups = groups_below(tree.root());
        let ws = &mut self.session.workspace;
        let mut shown = Vec::new();
        let mut present = Vec::new();
        for (node, panels, _) in groups {
            let tabs: Vec<TabId> = panels
                .iter()
                .filter_map(|p| self.map.tabs.get(p).copied())
                .collect();
            if tabs.is_empty() {
                continue;
            }
            present.push(node);
            let panel = *self
                .map
                .groups
                .entry(node)
                .or_insert_with(|| ws.new_panel_id());
            shown.push((panel, tabs));
        }
        self.map.groups.retain(|n, _| present.contains(n));
        shown.sort();
        if shown != self.shown {
            ws.sync_panels(shown.clone());
            self.shown = shown;
            let live: Vec<TabId> = ws.tabs().map(|t| t.id).collect();
            self.map.views.retain(|t, _| live.contains(t));
            self.map.tabs.retain(|_, t| live.contains(t));
        }
        ws.arrangement = layout_node(tree.root(), &self.map);
        if self.save_session
            && let Err(e) = self.session.save_last_session()
        {
            self.toast = Some(format!("Could not save the session: {e}"));
        }
    }

    /// The + menu, the Layout-name prompt and the toast, over the Workspace.
    pub(crate) fn overlays(
        &mut self,
        look: &Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some(node) = self.plus_menu {
            let panel = self.map.groups.get(&node).map(|p| p.0);
            let mut menu = components::menu("Open in this Panel", look)
                .absolute()
                .top(px(look.pt(tokens::TITLE_BAR_HEIGHT) + look.pt(size::ROW)))
                .right(px(look.pt(space::L)));
            let entries: Vec<(String, String)> = self
                .session
                .library
                .entries()
                .iter()
                .map(|e| (e.manifest.resource_id.clone(), e.manifest.title.clone()))
                .collect();
            for (id, title) in entries {
                let id2 = id.clone();
                menu = menu.child(
                    components::menu_item(
                        SharedString::from(format!("open-{id}")),
                        format!("Open {title}"),
                        None,
                        look,
                    )
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.plus_menu = None;
                        s.run_in(cx, "resource.open", &json!({ "id": id2, "panel": panel }));
                    })),
                );
            }
            menu = menu
                .child(
                    components::menu_item("open-library", "Open the Library", None, look).on_click(
                        cx.listener(move |s, _, _, cx| {
                            s.plus_menu = None;
                            s.run_in(cx, "library.open", &json!({ "panel": panel }));
                        }),
                    ),
                )
                .child(
                    components::menu_item("plus-cancel", "Cancel", None, look).on_click(
                        cx.listener(|s, _, _, cx| {
                            s.plus_menu = None;
                            cx.notify();
                        }),
                    ),
                );
            out.push(menu.into_any_element());
        }
        if let Some((rename, state)) = self.layout_prompt.clone() {
            let title = if rename.is_some() {
                "Rename Layout"
            } else {
                "Save Layout"
            };
            let field =
                components::text_field("layout-name", &state, "Layout name", look, window, cx);
            let rename2 = rename.clone();
            out.push(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        components::dialog(title, look).child(field).child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(look.pt(space::S)))
                                .child(
                                    components::primary_button("layout-save", "Save", look)
                                        .on_click(cx.listener(move |s, _, _, cx| {
                                            let name = state.read(cx).value().to_string();
                                            s.layout_prompt = None;
                                            match &rename2 {
                                                Some(old) => s.run_in(
                                                    cx,
                                                    "layout.rename",
                                                    &json!({ "name": old, "to": name }),
                                                ),
                                                None => s.run_in(
                                                    cx,
                                                    "layout.save",
                                                    &json!({ "name": name }),
                                                ),
                                            };
                                        })),
                                )
                                .child(
                                    components::button("layout-cancel", "Cancel", look).on_click(
                                        cx.listener(|s, _, _, cx| {
                                            s.layout_prompt = None;
                                            cx.notify();
                                        }),
                                    ),
                                ),
                        ),
                    )
                    .into_any_element(),
            );
        }
        if let Some(m) = &self.toast {
            out.push(
                div()
                    .absolute()
                    .bottom(px(look.pt(space::XL)))
                    .left(px(look.pt(space::XL)))
                    .child(components::toast(m.clone(), look))
                    .into_any_element(),
            );
        }
        out
    }
}

/// The share of the window's width a narrow Panel keeps for its header: the reference box
/// shrinks before the title (the egui `narrow_panels` rule). Exposed for tests.
pub fn min_panel_width(look: &Look) -> f32 {
    look.pt(size::FIELD) / 2.0
}
