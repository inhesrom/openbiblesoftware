//! Floating windows (#152): Tabs floated out of the main window into OS windows of their own,
//! each holding its own Panels. The Workspace keeps them (see [`crate::Workspace::float`]);
//! Layouts and the Last session save each one with where it was on screen, and a window whose
//! screen is gone when it is restored is moved onto one that is there
//! ([`WindowPlacement::on_screen`]).

use serde::{Deserialize, Serialize};

use crate::persist::LayoutNode;

/// A floating window, stable for the life of the Workspace (and kept in saved snapshots).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

/// A floating window as the Workspace knows it: its Panels and where it is on screen. Like
/// [`crate::Workspace::arrangement`] for the main window, the UI keeps it up to date.
#[derive(Debug, Clone, PartialEq)]
pub struct FloatingWindow {
    pub id: WindowId,
    pub arrangement: Option<LayoutNode>,
    pub placement: Option<WindowPlacement>,
}

/// A screen (monitor): its rectangle in desktop units and how many desktop units one egui
/// point takes on it. Desktop units are the operating system's own window coordinates, which
/// are the same across all its monitors: pixels on Windows and Linux, points on macOS. On a
/// desktop whose monitors have different scale factors, egui points are not: a window's points
/// are its own pixels divided by its own scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Desktop units per egui point on this screen (its scale factor where desktop units are
    /// pixels; 1 where they are points).
    #[serde(default = "unit_scale")]
    pub scale: f32,
}

fn unit_scale() -> f32 {
    1.0
}

impl ScreenRect {
    /// [`Self::scale`], never zero or negative (a hand-edited or damaged snapshot).
    pub fn units_per_point(&self) -> f32 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }

    /// Whether a point in desktop units is on this screen.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn overlap(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
        let w = (self.x + self.width).min(x + width) - self.x.max(x);
        let h = (self.y + self.height).min(y + height) - self.y.max(y);
        (w.max(0.0), h.max(0.0))
    }
}

/// Where a floating window is: its outer top-left corner in desktop units (see [`ScreenRect`]),
/// its inner size in egui points, and the screen it was on when that is known.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<ScreenRect>,
}

/// How much of a window's title bar must be on a screen for the user to grab it, in points.
const GRAB_WIDTH: f32 = 48.0;
const GRAB_HEIGHT: f32 = 16.0;
/// The title-bar strip checked against the screens, in points.
const TITLE_BAR: f32 = 32.0;
/// The smallest a floating window is made, in points.
pub const MIN_WINDOW: (f32, f32) = (240.0, 160.0);

impl WindowPlacement {
    /// Put the window on one of `screens` (those connected now; the main window's first) if
    /// too little of its title bar is on any of them to grab it, as when the screen it was on
    /// is gone. It keeps its place relative to the corner of the screen it was on, in points,
    /// as far as it fits on the new one, and shrinks to fit. With no screens known it stays as
    /// saved.
    pub fn on_screen(self, screens: &[ScreenRect]) -> Self {
        // The title bar as it would be on each screen, at that screen's scale.
        let reachable = screens.iter().any(|s| {
            let k = s.units_per_point();
            let (w, h) = s.overlap(self.x, self.y, self.width * k, TITLE_BAR * k);
            w >= GRAB_WIDTH.min(self.width) * k && h >= GRAB_HEIGHT * k
        });
        let Some(target) = screens.first().copied() else {
            return self;
        };
        if reachable {
            return self;
        }
        let k = target.units_per_point();
        // The new screen's size in points.
        let (room_w, room_h) = (target.width / k, target.height / k);
        let width = self.width.min(room_w).max(MIN_WINDOW.0.min(room_w));
        let height = self.height.min(room_h).max(MIN_WINDOW.1.min(room_h));
        // The offset from the old screen's corner, in points.
        let (dx, dy) = match self.screen {
            Some(old) => {
                let old_k = old.units_per_point();
                ((self.x - old.x) / old_k, (self.y - old.y) / old_k)
            }
            None => ((room_w - width) / 2.0, (room_h - height) / 2.0),
        };
        Self {
            x: target.x + dx.clamp(0.0, (room_w - width).max(0.0)) * k,
            y: target.y + dy.clamp(0.0, (room_h - height).max(0.0)) * k,
            width,
            height,
            screen: Some(target),
        }
    }

    /// Desktop units per egui point where the window is (1 when its screen is not known).
    pub fn units_per_point(&self) -> f32 {
        self.screen.map_or(1.0, |s| s.units_per_point())
    }
}

/// A floating window as saved in a Layout or the Last session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: u64,
    pub layout: LayoutNode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<WindowPlacement>,
}
