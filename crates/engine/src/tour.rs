//! The First-run tour (#218): a few skippable Coach marks, each pointing at one part of the
//! window (the toolbar, the Library, a Link set, the Passage Guide, Word Study on a right-click,
//! Home) with a line on what it is for. It runs at the first launch with a User database that
//! has never shown it, and never again unless the user replays it (Help → Take the tour).
//! Whether it has been shown is one preference, `tour`, kept in the User database.

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, Params, always};
use crate::session::Session;

/// Preference key of the First-run tour.
pub(crate) const TOUR_PREF: &str = "tour";

/// The part of the window a Coach mark points at. The UI finds where it is drawn; one that is
/// not on screen (the toolbar hidden, no Bible open) has its Coach mark in the middle of the
/// window instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TourTarget {
    /// The toolbar with its Launchers.
    Toolbar,
    /// The Library Launcher.
    Library,
    /// The Link set chip of a Bible Tab's header.
    LinkSets,
    /// The Guides Launcher, where the Passage Guide is.
    PassageGuide,
    /// A Bible's text, where a right-click on a word offers Word Study.
    WordStudy,
    /// The menu bar's Home button.
    Home,
}

/// One Coach mark of the First-run tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourStep {
    pub target: TourTarget,
    pub title: &'static str,
    pub text: &'static str,
}

/// The Coach marks, in order.
pub const TOUR_STEPS: &[TourStep] = &[
    TourStep {
        target: TourTarget::Toolbar,
        title: "The toolbar",
        text: "The menus, the Launchers and the Command box. Type a Bible reference such as \
               Jn 3:16 in the Command box to go there, or a word to search for it.",
    },
    TourStep {
        target: TourTarget::Library,
        title: "Your Library",
        text: "Every Bible, Commentary, Lexicon and Dictionary you have. Click one to open it \
               in a Tab; File → Get Resources… finds more.",
    },
    TourStep {
        target: TourTarget::LinkSets,
        title: "Link sets",
        text: "Tabs with the same letter follow each other: move one to a Passage and the \
               others go there too. Click the letter to change it.",
    },
    TourStep {
        target: TourTarget::PassageGuide,
        title: "Passage Guide",
        text: "Guides → Passage Guide gathers Commentaries, Cross-references, Key words and \
               People & places for the Passage you are reading.",
    },
    TourStep {
        target: TourTarget::WordStudy,
        title: "Word Study",
        text: "Right-click a word in a Bible with Strong's numbers and choose Word Study: its \
               Lexicon entries, how it is translated and where it occurs.",
    },
    TourStep {
        target: TourTarget::Home,
        title: "Home",
        text: "Today's reading, where you left off and Quick start. Help → User guide explains \
               everything, and Help → Take the tour shows this tour again.",
    },
];

/// The First-run tour's state: the Coach mark showing, if any, and whether the tour has been
/// shown (it then never starts by itself again).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tour {
    step: Option<usize>,
    shown: bool,
}

impl Tour {
    /// The Coach mark showing, with its index.
    pub fn step(&self) -> Option<(usize, &'static TourStep)> {
        let i = self.step?;
        Some((i, TOUR_STEPS.get(i)?))
    }

    pub fn is_running(&self) -> bool {
        self.step.is_some()
    }

    /// Whether the tour has been shown with this User database.
    pub fn shown(&self) -> bool {
        self.shown
    }
}

impl Session {
    pub fn tour(&self) -> &Tour {
        &self.tour
    }

    /// Read whether the tour has been shown from the User database (not, where it has no
    /// answer).
    pub(crate) fn load_tour(&mut self) {
        self.tour.shown = (self.user.as_ref())
            .and_then(|u| u.preference(TOUR_PREF).ok().flatten())
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .and_then(|v| v.get("shown").and_then(Value::as_bool))
            .unwrap_or(false);
    }

    /// At launch: start the tour when this User database has never shown it. Without a User
    /// database nothing could remember it, so it does not start. Returns whether it started.
    pub fn start_tour_at_first_launch(&mut self) -> bool {
        if self.user.is_none() || self.tour.shown {
            return false;
        }
        self.start_tour("tour.start").is_ok()
    }

    /// The Coach marks about a Bible (Link sets, Word Study) need one in view: when the focused
    /// Tab is not a Bible (Home, at the first launch), the most recent Bible Tab is shown.
    fn show_target(&mut self) {
        let Some((_, step)) = self.tour.step() else {
            return;
        };
        if !matches!(step.target, TourTarget::LinkSets | TourTarget::WordStudy) {
            return;
        }
        let library = &self.library;
        let bible = |t: &crate::workspace::Tab| match &t.kind {
            crate::workspace::TabKind::Resource(id) => library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == obs_model::ResourceType::Bible),
            _ => false,
        };
        let focused = (self.workspace.focused()).and_then(|t| self.workspace.tab(t));
        if focused.is_some_and(bible) {
            return;
        }
        if let Some(tab) = self.workspace.most_recent(bible) {
            self.workspace.focus(tab);
        }
    }

    /// Show the first Coach mark, and remember the tour as shown: it never starts by itself
    /// again, even when the app is closed during it.
    /// Saved first: a tour that could not be remembered as shown does not start.
    fn start_tour(&mut self, command: &'static str) -> Result<(), CommandError> {
        if let Some(user) = &self.user {
            user.set_preference(TOUR_PREF, &json!({ "shown": true }).to_string())
                .map_err(|e| CommandError::Failed {
                    command,
                    message: format!("saving that the tour was shown: {e}"),
                })?;
        }
        self.tour.step = Some(0);
        self.tour.shown = true;
        Ok(())
    }
}

// -------------------------------------------------------------------------------- commands

fn step_json(s: &Session) -> Value {
    match s.tour.step() {
        Some((i, step)) => json!({ "step": i + 1, "of": TOUR_STEPS.len(), "title": step.title }),
        None => json!({ "step": Value::Null, "of": TOUR_STEPS.len() }),
    }
}

fn replay(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.start_tour("help.tour")?;
    Ok(step_json(s))
}

fn next(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.tour.step = match s.tour.step {
        Some(i) if i + 1 < TOUR_STEPS.len() => Some(i + 1),
        _ => None,
    };
    s.show_target();
    Ok(step_json(s))
}

fn back(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    if let Some(i) = s.tour.step {
        s.tour.step = Some(i.saturating_sub(1));
    }
    s.show_target();
    Ok(step_json(s))
}

fn skip(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.tour.step = None;
    Ok(step_json(s))
}

const fn tour_cmd(
    id: &'static str,
    label: &'static str,
    run: fn(&mut Session, Params<'_>) -> Result<Value, CommandError>,
) -> CommandSpec {
    CommandSpec {
        id,
        label,
        shortcut: None,
        params: &[],
        enabled: always,
        run,
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    tour_cmd("help.tour", "Take the tour", replay),
    tour_cmd("tour.next", "Next step of the tour", next),
    tour_cmd("tour.back", "Previous step of the tour", back),
    tour_cmd("tour.skip", "Skip the tour", skip),
];
