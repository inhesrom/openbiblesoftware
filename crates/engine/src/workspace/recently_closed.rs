//! Recently closed (#214): the Tabs and Panels the user closed lately, newest last, so Reopen
//! closed Tab (Ctrl+Shift+T) brings them back in their old Panel at their Current location,
//! in their Link set and scrolled where they were. Each is kept as the same [`TabSnapshot`] a
//! Layout saves. The list lives for the session and is saved with the Last session, never
//! with a Layout.

use serde::{Deserialize, Serialize};

use super::{Effect, OpenRequest, PanelId, PendingVisit, Placement, Side, Tab, TabId, Workspace};
use crate::persist::{LayoutNode, ScrollPosition, TabSnapshot};

/// Recently closed keeps at most this many closings.
pub const RECENTLY_CLOSED_LIMIT: usize = 20;

/// One closing in Recently closed: a Tab, or the Tabs of a Panel closed at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTabs {
    /// Stable while it is in the list, to reopen it by.
    pub id: u64,
    /// The Tabs as they were, in their Panel's order.
    pub tabs: Vec<TabSnapshot>,
    /// Which of `tabs` was the Panel's active Tab (its saved id).
    pub active: Option<u64>,
    /// The Panel they were in; `None` once the Workspace was replaced by a Layout, whose
    /// Panels are others.
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

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl ClosedTabs {
    /// Whether this closed a Panel of several Tabs at once.
    pub fn is_panel(&self) -> bool {
        self.tabs.len() > 1
    }
}

impl Workspace {
    /// Recently closed, newest first.
    pub fn recently_closed(&self) -> impl Iterator<Item = &ClosedTabs> {
        self.closed.iter().rev()
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
        for t in tabs {
            self.close(t);
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

    /// Put `tabs` of `panel`, about to close, into Recently closed.
    fn remember(&mut self, panel: PanelId, tabs: &[TabId]) {
        let snapshots: Vec<TabSnapshot> = tabs
            .iter()
            .filter_map(|t| self.tabs.get(t))
            .map(TabSnapshot::of)
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
        self.next_closed += 1;
        self.closed.push(ClosedTabs {
            id: self.next_closed,
            tabs: snapshots,
            active,
            panel: Some(panel.0),
            beside,
            side,
            levels,
        });
        if self.closed.len() > RECENTLY_CLOSED_LIMIT {
            self.closed.remove(0);
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

    /// Where a closed Tab's view was scrolled, as the UI reads it while the Tab closes: kept
    /// with the newest closing that holds it.
    pub fn closed_scroll(&mut self, tab: TabId, at: ScrollPosition) {
        if let Some(t) = (self.closed.iter_mut().rev())
            .flat_map(|c| c.tabs.iter_mut())
            .find(|t| t.id == tab.0)
        {
            t.scroll = Some(at);
        }
    }

    /// Reopen a closing from Recently closed (`id`, or the newest): its Tabs come back in
    /// their old Panel, or in a Panel of their own where theirs closed with them, each at its
    /// Current location and in its Link set, the active one focused. A single-instance Tab
    /// already open again is focused instead (when it was the active one, or the first).
    /// Returns the Tabs reopened.
    pub fn reopen_closed(&mut self, id: Option<u64>) -> Option<Vec<TabId>> {
        let i = match id {
            Some(id) => self.closed.iter().position(|c| c.id == id)?,
            None => self.closed.len().checked_sub(1)?,
        };
        let mut closed = self.closed.remove(i);
        let mut placement: Option<Placement> = None;
        let mut reopened = Vec::new();
        let mut focus = None;
        for snap in std::mem::take(&mut closed.tabs) {
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
                None => self.closed_placement(&snap, &closed),
            };
            let panel = match here {
                Placement::Panel(p) | Placement::NewPanel { panel: p, .. } => p,
            };
            if placement.is_none() {
                self.came_back(panel, &closed);
            }
            // The rest of a closed Panel's Tabs go into the Panel the first one opened.
            placement = Some(Placement::Panel(panel));
            self.next_tab += 1;
            let tab = TabId(self.next_tab);
            let scroll = snap.scroll.clone();
            let entry = self.panels.entry(panel).or_default();
            entry.0.push(tab);
            entry.1 = Some(tab);
            self.tabs.insert(tab, Tab::from_snapshot(tab, snap));
            self.effects.push(Effect::Opened {
                tab,
                placement: here,
            });
            if let Some(at) = scroll {
                self.effects.push(Effect::RestoreScroll { tab, at });
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

    /// Panel `panel` came back without the Panel it was next to, which closed after it: that
    /// Panel's closing learns it is to come back beside this one, on the other side.
    fn came_back(&mut self, panel: PanelId, closed: &ClosedTabs) {
        let Some(missing) = closed
            .beside
            .filter(|b| !self.panels.contains_key(&PanelId(*b)))
        else {
            return;
        };
        let live = |p: &Option<u64>| p.is_some_and(|p| self.panels.contains_key(&PanelId(p)));
        let unplaced: Vec<usize> = (self.closed.iter().enumerate())
            .filter(|(_, c)| c.panel == Some(missing) && !live(&c.beside))
            .map(|(i, _)| i)
            .collect();
        for i in unplaced {
            let c = &mut self.closed[i];
            c.beside = Some(panel.0);
            c.side = closed.side.opposite();
            c.levels = 0;
        }
    }

    /// Where the first Tab of a closing goes: its old Panel while it is open, else that Panel
    /// again (by its old id, so Tabs closed from it one by one come back together), beside
    /// the one it was next to; after a Layout replaced the Workspace, wherever a Tab of its
    /// kind opens.
    fn closed_placement(&mut self, snap: &TabSnapshot, closed: &ClosedTabs) -> Placement {
        let live = |p: &PanelId| self.panels.contains_key(p);
        match closed.panel.map(PanelId) {
            Some(p) if live(&p) => Placement::Panel(p),
            Some(p) => match closed.beside.map(PanelId).filter(live) {
                Some(beside) => Placement::NewPanel {
                    panel: p,
                    beside: Some(beside),
                    side: closed.side,
                    levels: closed.levels,
                },
                None => Placement::right_of(p, None),
            },
            None => self.placement_for(&snap.kind, OpenRequest::default()),
        }
    }

    /// A Resource left the library: its closed Tabs cannot come back.
    pub fn forget_closed_resource(&mut self, id: &str) {
        for c in &mut self.closed {
            c.tabs.retain(|t| t.kind.resource_id() != Some(id));
        }
        self.closed.retain(|c| !c.tabs.is_empty());
    }

    /// Recently closed as the Workspace is replaced by a snapshot: the Last session's own
    /// list, else the current one, whose Panels are no longer there.
    pub(super) fn restore_closed(&mut self, saved: Vec<ClosedTabs>) {
        if saved.is_empty() {
            self.forget_closed_panels();
        } else {
            self.closed = saved;
        }
        let ids = self.closed.iter().map(|c| c.id);
        self.next_closed = ids.fold(self.next_closed, u64::max);
        // New Tabs and Panels never take ids the list still names.
        let tab_ids = self.closed.iter().flat_map(|c| c.tabs.iter().map(|t| t.id));
        self.next_tab = tab_ids.fold(self.next_tab, u64::max);
        let panel_ids = self
            .closed
            .iter()
            .flat_map(|c| c.panel.into_iter().chain(c.beside));
        self.next_panel = panel_ids.fold(self.next_panel, u64::max);
    }

    /// The Panels Recently closed names are gone with the arrangement they were in: its Tabs
    /// reopen where Tabs of their kind open.
    pub(super) fn forget_closed_panels(&mut self) {
        for c in &mut self.closed {
            c.panel = None;
            c.beside = None;
            c.side = Side::Right;
            c.levels = 0;
        }
    }

    /// Recently closed as the Last session saves it.
    pub(crate) fn closed_snapshot(&self) -> Vec<ClosedTabs> {
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
