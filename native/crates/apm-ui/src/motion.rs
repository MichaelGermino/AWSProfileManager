//! Motion: short, quiet transitions shared by every view.
//!
//! All of it goes through GPUI's animation elements, which honour `App::reduce_motion` (Windows
//! "Animation effects" off, macOS "Reduce motion"; gpui-component reads the setting at start-up and
//! the shell re-reads it when the window is activated): one-shot motion then shows its end state
//! and repeating motion its resting state, with no frames scheduled.
//!
//! One-shot animations stop asking for frames when they finish, so an idle window stays idle.
//! Repeating ones (the refresh spinner, the pulsing status dot) only exist while something is busy,
//! and run at a capped frame rate.
//!
//! GPUI has no transform for boxes, so "scale in" is a fade with a short rise; icons (SVG) can
//! rotate.

use std::time::Duration;

use gpui::{px, radians, Animation, AnimationElement, AnimationExt, ElementId, IntoElement, SpringAnimation, SpringConfig, Styled, Svg, Transformation};
use gpui_component::animation::ease_out_cubic;

/// Dialogs, the lock card, the tour card: fade in while rising 8px.
pub const DIALOG: Duration = Duration::from_millis(160);
/// Menus and pickers: fade in while sliding 4px.
pub const POPOVER: Duration = Duration::from_millis(120);
/// Toasts.
pub const TOAST: Duration = Duration::from_millis(180);

/// A dialog card (or the lock card) appearing.
pub fn dialog_in<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(DIALOG).with_easing(ease_out_cubic), |e, t| e.opacity(t).top(px(8. * (1. - t))))
}

/// A backdrop (scrim) fading in.
pub fn fade_in<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>, duration: Duration) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(duration).with_easing(ease_out_cubic), |e, t| e.opacity(t))
}

/// A menu or a picker list dropping open: fade in while sliding down 4px.
pub fn popover_in<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(POPOVER).with_easing(ease_out_cubic), |e, t| e.opacity(t).top(px(-4. * (1. - t))))
}

/// A toast arriving: slide up 12px and fade in.
pub fn toast_in<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(TOAST).with_easing(ease_out_cubic), |e, t| e.opacity(t).mb(px(-12. * (1. - t))))
}

/// A toast leaving: fade out while sinking 6px.
pub fn toast_out<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(TOAST).with_easing(ease_out_cubic), |e, t| e.opacity(1. - t).mb(px(-6. * t)))
}

/// An icon turning while its action runs (refresh). Calm: one turn a second, 30 frames a second.
pub fn spin(icon: Svg, id: impl Into<ElementId>) -> AnimationElement<Svg> {
    icon.with_animation(id, Animation::new(Duration::from_millis(1000)).repeat().with_max_fps(30.), |s, t| {
        s.with_transformation(Transformation::rotate(radians(t * std::f32::consts::TAU)))
    })
}

/// A soft pulse (the status dot while a profile refreshes).
pub fn pulse<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    el.with_animation(
        id,
        Animation::new(Duration::from_millis(1400)).repeat().with_easing(gpui::pulsating_between(0.35, 1.0)).with_max_fps(20.),
        |e, t| e.opacity(t),
    )
}

/// A disclosure chevron: points right when closed and turns to point down when open. The turn
/// animates when `open` changes; on first render it starts where it is.
pub fn chevron(icon: Svg, id: impl Into<ElementId>, open: bool) -> impl IntoElement {
    // Critically damped: no overshoot, settles in about a fifth of a second.
    let spring = SpringAnimation::new(SpringConfig::new(500., 2. * 500f32.sqrt(), 1.)).to(if open { 1.0f32 } else { 0.0 });
    icon.with_spring(id, spring, |s, t| s.with_transformation(Transformation::rotate(radians(t * std::f32::consts::FRAC_PI_2))))
}
