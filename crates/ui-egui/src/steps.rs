//! `OBS_STEPS` automation (the update e2e, #276): steps that run in order, each once what it
//! waits for is on screen. A frame script (`OBS_INPUT`) has to know in advance at which
//! frame and where a button will be; a step finds the button by its label in the frame's
//! accessibility tree and clicks its centre with the same pointer events, queued on the
//! `OBS_INPUT` queue.

use std::collections::VecDeque;
use std::path::PathBuf;

/// One step of `OBS_STEPS` (a JSON array of them). It runs once `when` is shown, `click`'s
/// button is shown, `after_file` exists and `after_secs` have passed since the step before
/// ran, and all of that has held, in the same place, for [`SETTLE_FRAMES`] frames and
/// [`SETTLE_SECS`]; then it
/// saves `shot` of that frame and clicks. It is dropped without running once `skip_if` is
/// shown.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Step {
    /// Text that must be on screen: part of a widget's label or a label's text.
    pub(crate) when: Option<String>,
    /// The exact label of an enabled button to click.
    pub(crate) click: Option<String>,
    /// A file that must exist first: the go-ahead of whatever drives the app.
    pub(crate) after_file: Option<PathBuf>,
    /// Seconds to wait after the step before ran (or the first frame).
    pub(crate) after_secs: f64,
    /// Save a screenshot (PPM) of the frame the step runs in; the app keeps running.
    pub(crate) shot: Option<PathBuf>,
    /// Text that, once shown, means the moment for this step has passed.
    pub(crate) skip_if: Option<String>,
}

/// How many frames in a row, and for how long, a step's conditions must hold, its button in
/// the same place, before it runs: a window is laid out invisibly first, may move as it
/// settles, and fades in.
pub(crate) const SETTLE_FRAMES: u32 = 3;
pub(crate) const SETTLE_SECS: f64 = 0.4;

/// The steps still to run.
#[derive(Debug, Default)]
pub(crate) struct Steps {
    queue: VecDeque<Step>,
    /// When the step before ran (egui time).
    since: Option<f64>,
    /// The last frame of the step before's click; nothing runs before it is done.
    busy_until: u32,
    /// For how many frames in a row the next step's conditions have held, since when, and
    /// where its button was.
    settled: (u32, f64, Option<egui::Rect>),
}

impl Steps {
    pub(crate) fn parse(json: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            queue: serde_json::from_str(json)?,
            ..Self::default()
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// At the end of frame `frame`'s UI: run the next step if it is due. Its click goes on
    /// `input` (pointer moved, pressed, released over the next three frames); its
    /// screenshot path is returned.
    pub(crate) fn run_due(
        &mut self,
        ctx: &egui::Context,
        frame: u32,
        input: &mut Vec<(u32, egui::Event)>,
    ) -> Option<PathBuf> {
        // The tree is built from the frame after this is first called.
        ctx.enable_accesskit();
        let now = ctx.input(|i| i.time);
        let since = *self.since.get_or_insert(now);
        let step = self.queue.front()?;
        if step
            .skip_if
            .as_ref()
            .is_some_and(|t| on_screen(ctx, t).is_some())
        {
            self.queue.pop_front();
            self.settled = (0, now, None);
            return None;
        }
        let target = step
            .click
            .as_ref()
            .map(|label| button_on_screen(ctx, label));
        let ready = frame > self.busy_until
            && now - since >= step.after_secs
            && !step.after_file.as_ref().is_some_and(|f| !f.exists())
            && !step
                .when
                .as_ref()
                .is_some_and(|t| on_screen(ctx, t).is_none())
            && !matches!(target, Some(None));
        let target = target.flatten();
        let same_place = match (self.settled.2, target) {
            (Some(a), Some(b)) => (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5,
            (None, None) => true,
            _ => false,
        };
        self.settled = match (ready, same_place, self.settled.0) {
            (false, _, _) => (0, now, None),
            (true, true, n) if n > 0 => (n + 1, self.settled.1, target),
            (true, _, _) => (1, now, target),
        };
        if self.settled.0 < SETTLE_FRAMES || now - self.settled.1 < SETTLE_SECS {
            return None;
        }
        self.settled = (0, now, None);
        let step = self.queue.pop_front()?;
        self.since = Some(now);
        if let Some(rect) = target {
            let pos = rect.center();
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            input.push((frame + 1, egui::Event::PointerMoved(pos)));
            input.push((frame + 2, button(true)));
            input.push((frame + 3, button(false)));
            self.busy_until = frame + 3;
            log::info!(
                "automation: clicking {:?} at ({:.0}, {:.0})",
                step.click.as_deref().unwrap_or_default(),
                pos.x,
                pos.y
            );
        }
        step.shot
    }
}

/// Where a widget showing `text` (part of its label or its text) is this frame.
pub(crate) fn on_screen(ctx: &egui::Context, text: &str) -> Option<egui::Rect> {
    find(ctx, |node| {
        node.label().is_some_and(|l| l.contains(text))
            || node.value().is_some_and(|v| v.contains(text))
    })
}

/// Where the enabled button labelled `label` is this frame.
pub(crate) fn button_on_screen(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
    find(ctx, |node| {
        node.role() == egui::accesskit::Role::Button
            && node.label() == Some(label)
            && !node.is_disabled()
    })
}

fn find(
    ctx: &egui::Context,
    matches: impl Fn(&egui::accesskit::Node) -> bool,
) -> Option<egui::Rect> {
    ctx.viewport(|v| {
        let nodes = &v.this_pass.accesskit_state.as_ref()?.nodes;
        nodes.values().filter(|n| matches(n)).find_map(|n| {
            let b = n.bounds()?;
            Some(egui::Rect::from_min_max(
                egui::pos2(b.x0 as f32, b.y0 as f32),
                egui::pos2(b.x1 as f32, b.y1 as f32),
            ))
        })
    })
}
