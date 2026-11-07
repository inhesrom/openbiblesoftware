//! The orders keyboard navigation moves in (#215): the Panels as the screen shows them, for
//! Ctrl+1…9, and a Panel's Tabs most recently used first, for the Tab switcher (Ctrl+Tab).

use super::{PanelId, TabId, Workspace};

impl Workspace {
    /// Every Panel in the order the screen shows them: the main window's left to right and
    /// top to bottom (each row or column in turn, as [`Workspace::arrangement`] nests them),
    /// then each Floating window's the same way, oldest window first. A Panel the UI has not
    /// placed yet comes last, by id.
    pub fn panels_in_order(&self) -> Vec<PanelId> {
        let placed = (self.arrangement.iter())
            .chain(self.floating.iter().filter_map(|w| w.arrangement.as_ref()))
            .flat_map(|node| node.panels())
            .map(|(panel, ..)| PanelId(panel));
        let mut out: Vec<PanelId> = Vec::new();
        for p in placed.chain(self.panels.keys().copied()) {
            if self.panels.contains_key(&p) && !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    /// The Tab switcher's list for `tab`: the Tabs of its Panel, `tab` first, then the others
    /// most recently active first (those never active in the Panel's order). Empty when `tab`
    /// is in no Panel.
    pub fn tab_switcher_order(&self, tab: TabId) -> Vec<TabId> {
        let Some((tabs, _)) = self.panel_of(tab).and_then(|p| self.panels.get(&p)) else {
            return Vec::new();
        };
        let mut others: Vec<TabId> = tabs.iter().copied().filter(|t| *t != tab).collect();
        others.sort_by_key(|t| std::cmp::Reverse(self.activity.get(t).copied().unwrap_or(0)));
        std::iter::once(tab).chain(others).collect()
    }
}
