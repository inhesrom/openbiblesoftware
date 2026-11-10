//! Motion (#225): smooth scrolling, eased jumps with a flash on the target verse, pop-ups that
//! fade and scale in, panels that ease to their new sizes and a shimmer on loading
//! placeholders. Every one is driven by frame time and timed by [`crate::tokens::motion`].
//!
//! **Reduce motion** is on when the user turns it on in the app (View → Reduce motion, its
//! Setting) or in the operating system ([`SystemMotion`]). Then nothing moves: scrolling takes
//! each wheel step at once, jumps land at once (the flash shows without fading), pop-ups and
//! panels appear at their final size, placeholders rest, and egui's own transitions take no
//! time. [`set`] installs the frame's choice; the rest read it back with [`reduced`].
//!
//! **Scroll speed** (#293) is the user's multiplier on wheel, trackpad and arrow-key scrolling;
//! [`scroll_input`] applies it (with Reduce motion's instant scrolling) once a frame in every
//! viewport.

use std::collections::HashSet;
use std::time::Duration;

use egui::emath::TSTransform;
use egui::{Context, Event, Id, LayerId, MouseWheelUnit, Order, Rect, Vec2, vec2};

use crate::tokens::motion;
use obs_engine::appearance::SCROLL_SPEED_DEFAULT;

/// The operating system's reduce motion setting, as the desktop app reads it.
pub trait SystemMotion: Send + Sync {
    /// Whether the user asked the system for less motion, as last read.
    fn reduce_motion(&self) -> bool;
    /// Read it again: the window came back to the front, so the user may have changed it.
    fn refresh(&self) {}
}

/// No system setting to read (the web, tests): motion is the app's choice alone.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSystemMotion;

impl SystemMotion for NoSystemMotion {
    fn reduce_motion(&self) -> bool {
        false
    }
}

fn reduced_id() -> Id {
    Id::unique("obs-reduce-motion")
}

fn scroll_speed_id() -> Id {
    Id::unique("obs-scroll-speed")
}

fn eased_layers_id() -> Id {
    Id::unique("obs-eased-popups")
}

/// Whether motion is reduced this frame.
pub fn reduced(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(reduced_id()))
        .unwrap_or(false)
}

/// Install this frame's motion: whether it is reduced, and egui's own transitions (fades,
/// collapsing sections, toggles, scroll-to) with it.
pub(crate) fn set(ctx: &Context, reduce: bool) {
    ctx.data_mut(|d| d.insert_temp(reduced_id(), reduce));
    let time = if reduce { 0.0 } else { motion::POPOVER as f32 };
    if ctx.global_style().animation_time != time {
        ctx.all_styles_mut(|s| {
            s.animation_time = time;
            s.scroll_animation = if reduce {
                egui::style::ScrollAnimation::none()
            } else {
                egui::style::ScrollAnimation::default()
            };
        });
    }
}

/// Install this frame's Scroll speed (#293): the multiplier [`scroll_input`] puts on wheel
/// and trackpad scrolling, and arrow keys on their steps.
pub(crate) fn set_scroll_speed(ctx: &Context, speed: f32) {
    ctx.data_mut(|d| d.insert_temp(scroll_speed_id(), speed));
}

/// This frame's Scroll speed (1×: egui's own steps).
pub fn scroll_speed(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(scroll_speed_id()))
        .unwrap_or(SCROLL_SPEED_DEFAULT)
}

/// This frame's wheel and trackpad scrolling, as every scrollable area reads it
/// (`smooth_scroll_delta`): at the Scroll speed, mouse-wheel lines and trackpad points alike.
/// With motion reduced it is also instant: this frame's input in whole, where egui spreads a
/// mouse-wheel step over the next frames. Ctrl+wheel zoom is left alone (egui turns it into
/// `zoom_delta`, never a scroll). Call at the start of a viewport's frame, before any widget
/// reads it; once per frame, in the main window and in each Floating window.
pub(crate) fn scroll_input(ctx: &Context) {
    let speed = scroll_speed(ctx);
    if !reduced(ctx) {
        if speed != SCROLL_SPEED_DEFAULT {
            ctx.input_mut(|i| i.smooth_scroll_delta *= speed);
        }
        return;
    }
    let (line, zoom, sideways) = ctx.options(|o| {
        let i = &o.input_options;
        (
            i.line_scroll_speed,
            i.zoom_modifier,
            i.horizontal_scroll_modifier,
        )
    });
    ctx.input_mut(|i| {
        let page = i.viewport_rect().height();
        let mut sum = Vec2::ZERO;
        for e in &i.events {
            if let Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } = e
            {
                if modifiers.matches_any(zoom) {
                    continue;
                }
                let d = match unit {
                    MouseWheelUnit::Point => *delta,
                    MouseWheelUnit::Line => line * *delta,
                    MouseWheelUnit::Page => page * *delta,
                };
                sum += if modifiers.matches_any(sideways) {
                    vec2(d.x + d.y, 0.0)
                } else {
                    d
                };
            }
        }
        i.smooth_scroll_delta = sum * speed;
    });
}

/// This frame's wheel and trackpad scrolling without the Scroll speed: what the Atlas and the
/// Timeline zoom by, so their wheel zoom is the same at every Scroll speed.
pub(crate) fn unscaled_scroll_delta(ctx: &Context) -> Vec2 {
    ctx.input(|i| i.smooth_scroll_delta) / scroll_speed(ctx)
}

/// How a pop-up that became visible at `since` is drawn at `now`: scaled up from
/// [`motion::POPOVER_SCALE`] about the middle of its top edge over [`motion::POPOVER`]
/// (`None`: at its own size). egui fades it in over the same time.
pub(crate) fn popup_transform(rect: Rect, since: f64, now: f64) -> Option<TSTransform> {
    if motion::is_done(now, since, motion::POPOVER) {
        return None;
    }
    let s = motion::popup_scale(motion::progress(now, since, motion::POPOVER));
    let pivot = rect.center_top().to_vec2();
    Some(
        TSTransform::from_translation(pivot)
            * TSTransform::from_scaling(s)
            * TSTransform::from_translation(-pivot),
    )
}

/// Ease the pop-ups (menus, popovers, the Command box's list, Launcher panes) of the viewport
/// being drawn: each scales in as [`popup_transform`] says. The scale is its layer's
/// transform, so the pointer meets what is drawn. Call at the end of a viewport's frame.
pub(crate) fn ease_popups(ctx: &Context) {
    let reduce = reduced(ctx);
    let now = ctx.input(|i| i.time);
    let layers: Vec<LayerId> = ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == Order::Foreground && m.areas().is_visible(l))
            .collect()
    });
    let mut eased: HashSet<LayerId> = ctx
        .data(|d| d.get_temp(eased_layers_id()))
        .unwrap_or_default();
    let mut moving = false;
    let mut now_eased = HashSet::new();
    for layer in layers {
        let transform = (!reduce)
            .then(|| AreaStateView::load(ctx, layer))
            .flatten()
            .and_then(|a| popup_transform(a.rect, a.since?, now));
        if let Some(t) = transform {
            ctx.set_transform_layer(layer, t);
            now_eased.insert(layer);
            moving = true;
        }
    }
    // Layers that finished (or were hidden) go back to their own size.
    for layer in eased.drain() {
        if !now_eased.contains(&layer) {
            ctx.set_transform_layer(layer, TSTransform::IDENTITY);
        }
    }
    ctx.data_mut(|d| d.insert_temp(eased_layers_id(), now_eased));
    if moving {
        ctx.request_repaint();
    }
}

/// What [`ease_popups`] needs of an egui Area's state.
struct AreaStateView {
    rect: Rect,
    since: Option<f64>,
}

impl AreaStateView {
    fn load(ctx: &Context, layer: LayerId) -> Option<Self> {
        let state = egui::AreaState::load(ctx, layer.id)?;
        Some(Self {
            rect: state.rect(),
            since: state.last_became_visible_at,
        })
    }
}

/// The colour of a loading placeholder at `y` (screen points) in `area`, this frame: `color`
/// with the shimmer over it, or `color` itself with motion reduced. A shimmer asks for the
/// next frame (at [`motion::SHIMMER_FRAME`]).
pub(crate) fn shimmer(ctx: &Context, area: Rect, y: f32, color: egui::Color32) -> egui::Color32 {
    if reduced(ctx) {
        return color;
    }
    let now = ctx.input(|i| i.time);
    ctx.request_repaint_after(Duration::from_secs_f64(motion::SHIMMER_FRAME));
    let at = (y - area.top()) / area.height().max(crate::tokens::space::PX);
    color.gamma_multiply(motion::shimmer_gain(at, now))
}
