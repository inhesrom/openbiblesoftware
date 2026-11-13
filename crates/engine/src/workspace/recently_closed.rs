//! Recently closed (#214): the Tabs and Panels the user closed lately, newest last, so Reopen
//! closed Tab (Ctrl+Shift+T) brings them back in their old Panel at their Current location,
//! in their Link set and scrolled where they were. Each closing is taken once, as the user
//! closes it, as the same [`TabSnapshot`] a Layout saves plus where its view was scrolled,
//! and is never changed after: what happens to the Workspace later (a Layout, Reset Layout,
//! another Tab taking its id) is kept beside the list, not written into it. The list lives
//! for the session and is saved with the Last session, never with a Layout.

use serde::{Deserialize, Serialize};

use super::{
    Effect, OpenRequest, PanelId, PendingVisit, Placement, Side, Tab, TabId, Workspace, after,
    first_free_id,
};
use crate::persist::{LayoutNode, TabSnapshot};

/// Recently closed keeps at most this many closings.
pub const RECENTLY_CLOSED_LIMIT: usize = 20;

/// One closing in Recently closed: a Tab, or the Tabs of a Panel closed at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTabs {
    /// Stable while it is in the list, to reopen it by.
    pub id: u64,
    /// The Tabs as they were, in their Panel's order, each scrolled as its view was.
    pub tabs: Vec<TabSnapshot>,
    /// Which of `tabs` was the Panel's active Tab (its saved id).
    pub active: Option<u64>,
    /// Which Panels `panel` and `beside` name: the [`RecentlyClosed::generation`] they were
    /// closed in. Once a Layout or Reset Layout has replaced the Panels, those ids name others.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub generation: u64,
    /// The Panel they were in.
    pub panel: Option<u64>,
    /// For a Panel that closed with them, what it was next to in its row or column (the
    /// Panel or the rows and columns before it, else after it): it comes back beside that,
    /// on the same side. This is that one's Panel nearest theirs.
    pub beside: Option<u64>,
    /// Which side of `beside` their Panel was on.
    #[serde(default, skip_serializing_if = "Side::is_right")]
    pub side: Side,
    /// How many rows and columns out from `beside` what their Panel was next to is: 0 when
    /// that is the Panel `beside` itself.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub levels: u8,
}

fn is_zero<N: Default + PartialEq>(n: &N) -> bool {
    *n == N::default()
}

impl ClosedTabs {
    /// Whether this closed a Panel of several Tabs at once.
    pub fn is_panel(&self) -> bool {
        self.tabs.len() > 1
    }
}

/// Recently closed as the Workspace keeps it and the Last session saves it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecentlyClosed {
    /// The closings, oldest first, at most [`RECENTLY_CLOSED_LIMIT`].
    pub closed: Vec<ClosedTabs>,
    /// Counts the times a Layout or Reset Layout replaced the Panels; a closing's Panel ids
    /// stand for Panels only while it is the generation they were closed in.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub generation: u64,
    /// Closed Panels whose neighbour came back without them: each comes back beside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub came_back_beside: Vec<CameBackBeside>,
}

impl RecentlyClosed {
    /// Nothing to reopen.
    pub fn is_empty(&self) -> bool {
        self.closed.is_empty()
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Panel `panel`, closed, comes back on `side` of Panel `beside`: the Panel it was next to,
/// which came back first without it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameBackBeside {
    pub panel: u64,
    pub beside: u64,
    pub side: Side,
}

impl Workspace {
    /// Recently closed, newest first.
    pub fn recently_closed(&self) -> impl Iterator<Item = &ClosedTabs> {
        self.closed.closed.iter().rev()
    }

    /// The user closed a Tab (Close Tab, Ctrl+W): it goes into Recently closed.
    pub fn user_closed(&mut self, tab: TabId) -> bool {
        let Some(panel) = self.panel_of(tab) else {
            return self.close(tab);
        };
        self.remember(panel, &[tab]);
        self.close(tab)
    }

    /// The user closed a whole Panel: its Tabs go into Recently closed as one closing, so
    /// Reopen closed Tab brings the Panel back with all of them.
    pub fn close_panel(&mut self, panel: PanelId) -> bool {
        let Some((tabs, _)) = self.panels.get(&panel).cloned() else {
            return false;
        };
        self.remember(panel, &tabs);
        let window = (self.focused())
            .filter(|f| tabs.contains(f))
            .map(|f| self.window_of(f));
        for t in tabs {
            self.close_without_refocus(t);
        }
        if let Some(window) = window {
            self.focus_after_close(panel, window);
        }
        true
    }

    /// Tabs the UI no longer shows were closed by the user: each Panel's go into Recently
    /// closed together.
    pub(super) fn remember_gone(&mut self, gone: &[TabId]) {
        let mut by_panel: Vec<(PanelId, Vec<TabId>)> = Vec::new();
        for (panel, (tabs, _)) in &self.panels {
            let closed: Vec<TabId> = tabs.iter().filter(|t| gone.contains(t)).copied().collect();
            if !closed.is_empty() {
                by_panel.push((*panel, closed));
            }
        }
        for (panel, tabs) in by_panel {
            self.remember(panel, &tabs);
        }
    }

    /// Put `tabs` of `panel`, about to close, into Recently closed, each scrolled where its
    /// view last was ([`Tab::scroll`]).
    fn remember(&mut self, panel: PanelId, tabs: &[TabId]) {
        let snapshots: Vec<TabSnapshot> = tabs
            .iter()
            .filter_map(|t| self.tabs.get(t))
            .map(TabSnapshot::closing)
            .collect();
        if snapshots.is_empty() {
            return;
        }
        let active = self
            .panels
            .get(&panel)
            .and_then(|(_, a)| *a)
            .filter(|a| tabs.contains(a))
            .map(|a| a.0);
        let (beside, side, levels) = match self.neighbour(panel) {
            Some((p, side, levels)) => (Some(p), side, levels),
            None => (None, Side::Right, 0),
        };
        let id = first_free_id(after(self.next_closed), |id| {
            self.closed.closed.iter().any(|c| c.id == id)
        });
        self.next_closed = id;
        let list = &mut self.closed;
        list.closed.push(ClosedTabs {
            id,
            tabs: snapshots,
            active,
            generation: list.generation,
            panel: Some(panel.0),
            beside,
            side,
            levels,
        });
        if list.closed.len() > RECENTLY_CLOSED_LIMIT {
            list.closed.remove(0);
        }
    }

    /// What `panel` is next to in its window's arrangement: see [`ClosedTabs::beside`].
    fn neighbour(&self, panel: PanelId) -> Option<(u64, Side, u8)> {
        std::iter::once(self.arrangement.as_ref())
            .chain(self.floating.iter().map(|w| w.arrangement.as_ref()))
            .flatten()
            .find_map(|a| neighbour_in(a, panel.0))
            .flatten()
    }

    /// Reopen a closing from Recently closed (`id`, or the newest): its Tabs come back in
    /// their old Panel, or in a Panel of their own where theirs closed with them, each at its
    /// Current location and in its Link set, the active one focused. A single-instance Tab
    /// already open again is focused instead (when it was the active one, or the first).
    /// Returns the Tabs reopened.
    pub fn reopen_closed(&mut self, id: Option<u64>) -> Option<Vec<TabId>> {
        let list = &mut self.closed.closed;
        let i = match id {
            Some(id) => list.iter().position(|c| c.id == id)?,
            None => list.len().checked_sub(1)?,
        };
        let closed = list.remove(i);
        let mut placement: Option<Placement> = None;
        let mut reopened = Vec::new();
        let mut focus = None;
        for snap in &closed.tabs {
            let was_active = closed.active == Some(snap.id);
            if snap.kind.single_instance()
                && let Some(open) = self.existing(&snap.kind)
            {
                if was_active || focus.is_none() {
                    focus = Some(open);
                }
                continue;
            }
            let here = match placement {
                Some(p) => p,
                None => self.closed_placement(snap, &closed),
            };
            let panel = match here {
                Placement::Panel(p) | Placement::NewPanel { panel: p, .. } => p,
            };
            if placement.is_none() {
                self.came_back(panel, &closed);
            }
            // The rest of a closed Panel's Tabs go into the Panel the first one opened.
            placement = Some(Placement::Panel(panel));
            let tab = self.take_tab_id();
            let entry = self.panels.entry(panel).or_default();
            entry.0.push(tab);
            entry.1 = Some(tab);
            self.tabs.insert(tab, Tab::from_snapshot(tab, snap.clone()));
            self.queue(Effect::Opened {
                tab,
                placement: here,
            });
            if let Some(at) = snap.scroll.clone() {
                self.queue(Effect::RestoreScroll { tab, at });
            }
            self.visits.push(PendingVisit { tab, at: None });
            if was_active || focus.is_none() {
                focus = Some(tab);
            }
            reopened.push(tab);
        }
        if let Some(t) = focus {
            self.focus(t);
        }
        Some(reopened)
    }

    /// The Panels a closing names, while they are still the ones it means.
    fn closed_panels(&self, closed: &ClosedTabs) -> Option<(PanelId, Option<PanelId>)> {
        let current = closed.generation == self.closed.generation;
        let panel = closed.panel.filter(|_| current)?;
        Some((PanelId(panel), closed.beside.map(PanelId)))
    }

    /// Panel `panel` came back for `closed`, without the Panel it was next to, which closed
    /// after it: that Panel is to come back beside this one, on the other side.
    fn came_back(&mut self, panel: PanelId, closed: &ClosedTabs) {
        let Some((_, Some(missing))) = self.closed_panels(closed) else {
            return;
        };
        if self.panels.contains_key(&missing) {
            return;
        }
        let list = &mut self.closed.came_back_beside;
        list.retain(|b| b.panel != missing.0);
        list.push(CameBackBeside {
            panel: missing.0,
            beside: panel.0,
            side: closed.side.opposite(),
        });
    }

    /// Where the first Tab of a closing goes: its old Panel while it is open, else that Panel
    /// again (by its old id, so Tabs closed from it one by one come back together), beside
    /// the one it was next to; after a Layout or Reset Layout replaced the Panels, wherever a
    /// Tab of its kind opens.
    fn closed_placement(&mut self, snap: &TabSnapshot, closed: &ClosedTabs) -> Placement {
        let Some((p, beside)) = self.closed_panels(closed) else {
            return self.placement_for(&snap.kind, OpenRequest::default());
        };
        let live = |p: &PanelId| self.panels.contains_key(p);
        if live(&p) {
            return Placement::Panel(p);
        }
        if let Some(beside) = beside.filter(live) {
            return Placement::NewPanel {
                panel: p,
                beside: Some(beside),
                side: closed.side,
                levels: closed.levels,
            };
        }
        let came_back = (self.closed.came_back_beside.iter())
            .find(|b| b.panel == p.0 && live(&PanelId(b.beside)));
        match came_back {
            Some(b) => Placement::NewPanel {
                panel: p,
                beside: Some(PanelId(b.beside)),
                side: b.side,
                levels: 0,
            },
            None => Placement::right_of(p, None),
        }
    }

    /// A Resource left the library: its closed Tabs cannot come back.
    pub fn forget_closed_resource(&mut self, id: &str) {
        for c in &mut self.closed.closed {
            c.tabs.retain(|t| t.kind.resource_id() != Some(id));
        }
        self.closed.closed.retain(|c| !c.tabs.is_empty());
    }

    /// Recently closed as the Workspace is replaced by a snapshot: the Last session's own
    /// list, else (a Layout) the current one, whose Panels are no longer there.
    pub(super) fn restore_closed(&mut self, saved: RecentlyClosed) {
        if saved.is_empty() {
            self.panels_replaced();
        } else {
            self.closed = saved;
        }
        let list = &self.closed.closed;
        self.next_closed = list.iter().map(|c| c.id).fold(self.next_closed, u64::max);
        // New Tabs and Panels never take ids the list still names.
        let tab_ids = list.iter().flat_map(|c| c.tabs.iter().map(|t| t.id));
        self.next_tab = tab_ids.fold(self.next_tab, u64::max);
        let panel_ids = list
            .iter()
            .flat_map(|c| c.panel.into_iter().chain(c.beside));
        self.next_panel = panel_ids.fold(self.next_panel, u64::max);
    }

    /// A Layout or Reset Layout replaced the Panels: the ones Recently closed names are gone,
    /// so its Tabs reopen where Tabs of their kind open.
    pub(super) fn panels_replaced(&mut self) {
        self.closed.generation += 1;
        self.closed.came_back_beside.clear();
    }

    /// Recently closed as the Last session saves it.
    pub(crate) fn closed_snapshot(&self) -> RecentlyClosed {
        self.closed.clone()
    }
}

/// Within `node`: `None` when it does not hold Panel `panel`, else what that Panel is next to
/// in the innermost row or column holding it with others (none: it is alone).
fn neighbour_in(node: &LayoutNode, panel: u64) -> Option<Option<(u64, Side, u8)>> {
    let (children, row) = match node {
        LayoutNode::Panel { panel: p, .. } => return (*p == panel).then_some(None),
        LayoutNode::Row { children } => (children, true),
        LayoutNode::Column { children } => (children, false),
    };
    let (i, inner) = (children.iter().enumerate())
        .find_map(|(i, (c, _))| neighbour_in(c, panel).map(|n| (i, n)))?;
    if inner.is_some() {
        return Some(inner);
    }
    let (side, next) = match i.checked_sub(1) {
        Some(b) => (if row { Side::Right } else { Side::Below }, b),
        None => (if row { Side::Left } else { Side::Above }, i + 1),
    };
    Some(children.get(next).map(|(c, _)| {
        let (p, levels) = nearest_panel(c, side.before());
        (p, side, levels)
    }))
}

/// The Panel of `node` nearest its start (`first`) or end, and how many rows and columns
/// deep it is in `node`.
fn nearest_panel(node: &LayoutNode, first: bool) -> (u64, u8) {
    match node {
        LayoutNode::Panel { panel, .. } => (*panel, 0),
        LayoutNode::Row { children } | LayoutNode::Column { children } => {
            let c = if first {
                children.first()
            } else {
                children.last()
            };
            c.map_or((0, 0), |(c, _)| {
                let (p, l) = nearest_panel(c, first);
                (p, l.saturating_add(1))
            })
        }
    }
}
