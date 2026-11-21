//! The design tokens the reading core measures with (#221, ADR 0015): the spacing scale, the
//! motion durations and easings, and the few sizes and reading-page marks the reading view
//! uses. `obs_ui_egui::tokens` re-exports each of them alongside the chrome's own tokens, so
//! there is still one place each value is written down; a test (`design_tokens_tests.rs` in
//! `obs-ui-egui`) fails on a spacing or size literal anywhere else in this crate too.

/// The spacing scale, in points: gaps between widgets, insets of frames, indents.
pub mod space {
    /// One point: a mark, a ring or a rect kept one point clear of an edge, a hairline or a
    /// neighbour.
    pub const PX: f32 = 1.0;
    /// Between a glyph and its mark; inside a chip.
    pub const XXS: f32 = 2.0;
    /// Between closely related controls.
    pub const XS: f32 = 4.0;
    /// Between the rows of a list; a control's inner gap.
    pub const S: f32 = 6.0;
    /// Between controls; the default item spacing.
    pub const M: f32 = 8.0;
    /// Between groups inside a card or a section.
    pub const L: f32 = 12.0;
    /// Between sections; a page's side inset.
    pub const XL: f32 = 16.0;
    /// Between major regions; the reading page's margin.
    pub const XXL: f32 = 24.0;
    /// Empty-state breathing room.
    pub const XXXL: f32 = 32.0;
}

/// Sizes of the reading page.
pub mod size {
    /// The narrowest a block of laid-out text is given, however small its Panel.
    pub const TEXT_MIN: f32 = 40.0;
}

/// Motion (#225): how long each kind of movement takes, in seconds, and how it eases. Every
/// animation is driven by frame time (egui's input time), never by a timer, and none runs
/// with Reduce motion on (the desktop's `obs_ui_egui::motion`): then every change is instant.
pub mod motion {
    /// A popover, menu or other pop-up fading and scaling in; egui's own transitions
    /// (collapsing sections, toggles, scroll-to) take this long too.
    pub const POPOVER: f64 = 0.12;
    /// How large a pop-up starts, about the middle of its top edge.
    pub const POPOVER_SCALE: f32 = 0.96;
    /// Panels settling into a new arrangement after a Tab is docked, split off, floated or
    /// closed: an outline easing from each Panel's old bounds to its new ones, a new Panel
    /// fading in.
    pub const PANEL: f64 = 0.18;
    /// A jump (a Link set follow, the reference box, Back) easing into place.
    pub const JUMP: f64 = 0.28;
    /// How far a jump glides at most, in heights of its pane: a longer jump lands that far
    /// short and eases the rest of the way.
    pub const JUMP_REACH: f32 = 1.0;
    /// A keyboard scroll (Page Down, Space, the arrows) easing to its place.
    pub const SCROLL: f64 = 0.14;
    /// The flash on a jump's target verse, fading out.
    pub const FLASH: f64 = 1.2;
    /// One sweep of the shimmer down a loading placeholder, and the most it brightens a bar
    /// (a multiple of the bar's own strength) as the sweep passes.
    pub const SHIMMER: f64 = 1.4;
    pub const SHIMMER_GAIN: f32 = 1.2;
    /// The shimmer's band, in heights of the area it sweeps.
    pub const SHIMMER_BAND: f32 = 0.35;
    /// How often a shimmer is drawn again while nothing else moves (30 a second).
    pub const SHIMMER_FRAME: f64 = 1.0 / 30.0;

    /// Ease out (cubic): fast at first, settling gently. Pop-ups, panels and jumps.
    pub fn ease_out(t: f32) -> f32 {
        let u = 1.0 - t.clamp(0.0, 1.0);
        1.0 - u * u * u
    }

    /// Ease in and out (cubic): a slow start and end. Keyboard scrolling.
    pub fn ease_in_out(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            let u = 2.0 - 2.0 * t;
            1.0 - u * u * u / 2.0
        }
    }

    /// How far through an animation of `duration` seconds that began at `start` frame time
    /// `now` is, from 0 to 1.
    pub fn progress(now: f64, start: f64, duration: f64) -> f32 {
        if duration <= 0.0 {
            return 1.0;
        }
        ((now - start) / duration).clamp(0.0, 1.0) as f32
    }

    /// Whether that animation has finished.
    pub fn is_done(now: f64, start: f64, duration: f64) -> bool {
        progress(now, start, duration) >= 1.0
    }

    /// What is left of that animation, from 1 down to 0: the strength of a fade-out.
    pub fn remaining(now: f64, start: f64, duration: f64) -> f32 {
        1.0 - progress(now, start, duration)
    }

    /// A pop-up's scale `p` of the way through its [`POPOVER`] time.
    pub fn popup_scale(p: f32) -> f32 {
        POPOVER_SCALE + (1.0 - POPOVER_SCALE) * ease_out(p)
    }

    /// How strong the shimmer makes a loading placeholder at height `y` (a fraction of the
    /// area it sweeps, 0 at its top) at frame time `now`: 1 at rest, up to 1 +
    /// [`SHIMMER_GAIN`] where the band passes. The band sweeps down the area once every
    /// [`SHIMMER`], from a band above its top to a band below its bottom.
    pub fn shimmer_gain(y: f32, now: f64) -> f32 {
        let phase = (now / SHIMMER).fract() as f32;
        let middle = -SHIMMER_BAND + phase * (1.0 + 2.0 * SHIMMER_BAND);
        let d = (y - middle).abs() / SHIMMER_BAND;
        if d >= 1.0 {
            1.0
        } else {
            1.0 + SHIMMER_GAIN * ease_in_out(1.0 - d)
        }
    }
}

/// Marks drawn on the reading page (#21, #63, #181), in ems of the Resource text size.
pub mod mark {
    /// A Footnote marker's box: the upper share of its line, where the raised glyph sits.
    pub const FOOTNOTE_BOX: f32 = 0.6;
}
