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

/// A rectangle on the desktop, in points: a window's or a screen's (monitor's).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ScreenRect {
    fn overlap(&self, other: &ScreenRect) -> (f32, f32) {
        let w = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let h = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        (w.max(0.0), h.max(0.0))
    }
}

/// Where a floating window is: its outer top-left corner and inner size on the desktop, in
/// points, and the screen it was on when that is known.
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
/// The title-bar strip checked against the screens.
const TITLE_BAR: f32 = 32.0;
/// The smallest a floating window is made, in points.
pub const MIN_WINDOW: (f32, f32) = (240.0, 160.0);

impl WindowPlacement {
    /// Put the window on one of `screens` (those connected now; the main window's first) if
    /// too little of its title bar is on any of them to grab it, as when the screen it was on
    /// is gone. It keeps its place relative to the corner of the screen it was on, as far as
    /// it fits on the new one, and shrinks to fit. With no screens known it stays as saved.
    pub fn on_screen(self, screens: &[ScreenRect]) -> Self {
        let title = ScreenRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: TITLE_BAR,
        };
        let reachable = screens.iter().any(|s| {
            let (w, h) = title.overlap(s);
            w >= GRAB_WIDTH.min(self.width) && h >= GRAB_HEIGHT
        });
        let Some(target) = screens.first().copied() else {
            return self;
        };
        if reachable {
            return self;
        }
        let width = self
            .width
            .min(target.width)
            .max(MIN_WINDOW.0.min(target.width));
        let height = self
            .height
            .min(target.height)
            .max(MIN_WINDOW.1.min(target.height));
        let (dx, dy) = match self.screen {
            Some(old) => (self.x - old.x, self.y - old.y),
            None => ((target.width - width) / 2.0, (target.height - height) / 2.0),
        };
        Self {
            x: target.x + dx.clamp(0.0, (target.width - width).max(0.0)),
            y: target.y + dy.clamp(0.0, (target.height - height).max(0.0)),
            width,
            height,
            screen: Some(target),
        }
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
