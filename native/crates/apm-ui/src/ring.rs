//! The expiry ring: the app's signature detail. A small ring beside each countdown that drains as
//! the session runs down (full when fresh), in the credential colours: green while active, amber in
//! the last 15 minutes, red once expired, a hollow grey ring when never signed in, and a short arc
//! turning calmly while the profile refreshes.
//!
//! Drawn with a GPUI quad (the round track) and one stroked path (the arc), so it is
//! crisp at any scale and costs nothing while idle: it only changes when the dashboard is re-read
//! (every 30 s) or a refresh starts. The turning arc is a repeating animation, so it follows the
//! reduced-motion setting like the other spinners (`motion.rs`) and exists only while busy.

use std::time::Duration;

use gpui::{
    canvas, point, prelude::*, px, quad, Animation, AnimationExt as _, App, BorderStyle, Bounds, ElementId, Hsla, IntoElement, PathBuilder, Rgba, Window,
};

use crate::status::CredState;
use crate::theme::Palette;

/// What one ring shows.
#[derive(Clone, Copy, IntoElement)]
pub struct Ring {
    diameter: f32,
    /// 0–1 of the session left (drawn as the arc), or `None` for no arc.
    fraction: Option<f32>,
    arc: Rgba,
    track: Rgba,
    /// Turning arc while refreshing: its start, in turns.
    spin: Option<f32>,
}

impl Ring {
    /// A ring for a credential state. `fraction` is the share of the session left (active states).
    pub fn new(state: CredState, fraction: f32, diameter: f32, p: &Palette) -> Self {
        let tint = |c: Rgba, a: f32| Rgba { a, ..c };
        let soft = if p.is_dark() { 0.2 } else { 0.18 };
        match state {
            CredState::Active => Ring { diameter, fraction: Some(fraction), arc: p.success, track: tint(p.success, soft), spin: None },
            CredState::Expiring => Ring { diameter, fraction: Some(fraction), arc: p.warning, track: tint(p.warning, soft), spin: None },
            CredState::Expired => Ring { diameter, fraction: None, arc: p.danger, track: p.danger, spin: None },
            CredState::Never => Ring { diameter, fraction: None, arc: p.text_faint, track: Rgba { a: 0.8, ..p.text_faint }, spin: None },
        }
    }

    /// The refreshing ring: a faint track with a quarter arc in the accent colour.
    pub fn refreshing(diameter: f32, p: &Palette) -> Self {
        Ring { diameter, fraction: Some(0.28), arc: p.accent, track: Rgba { a: 0.25, ..p.text_faint }, spin: Some(0.) }
    }

    fn stroke(&self) -> f32 {
        (self.diameter / 8.).clamp(1.5, 3.)
    }
}

impl RenderOnce for Ring {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let r = self;
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                let d = r.diameter;
                let w = r.stroke();
                let c = bounds.center();
                let (cx_, cy_) = (f32::from(c.x), f32::from(c.y));
                let ring_bounds = Bounds::centered_at(c, gpui::size(px(d), px(d)));
                // The track: a round quad with a border, which GPUI anti-aliases.
                window.paint_quad(quad(ring_bounds, px(d / 2.), gpui::transparent_black(), px(w), Hsla::from(r.track), BorderStyle::Solid));
                if let Some(f) = r.fraction.map(|f| f.clamp(0., 1.)) {
                    let start = r.spin.unwrap_or(0.) * std::f32::consts::TAU;
                    let sweep = f * std::f32::consts::TAU;
                    let rad = d / 2. - w / 2.;
                    let at = |a: f32| point(px(cx_ + rad * a.sin()), px(cy_ - rad * a.cos()));
                    if f >= 0.995 {
                        window.paint_quad(quad(ring_bounds, px(d / 2.), gpui::transparent_black(), px(w), Hsla::from(r.arc), BorderStyle::Solid));
                    } else if f > 0.01 {
                        let mut path = PathBuilder::stroke(px(w)).with_style(gpui::PathStyle::Stroke(
                            gpui::StrokeOptions::default().with_line_width(w).with_line_cap(lyon_tessellation::LineCap::Round),
                        ));
                        path.move_to(at(start));
                        let radii = point(px(rad), px(rad));
                        // Two halves past a half turn, so each arc stays well defined.
                        if sweep > std::f32::consts::PI {
                            path.arc_to(radii, px(0.), false, true, at(start + std::f32::consts::PI));
                            path.arc_to(radii, px(0.), false, true, at(start + sweep));
                        } else {
                            path.arc_to(radii, px(0.), false, true, at(start + sweep));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, r.arc);
                        }
                    }
                }
            },
        )
        .size(px(r.diameter))
        .flex_shrink_0()
    }
}

/// The ring for a profile: turning while it refreshes, else its state.
pub fn expiry_ring(id: impl Into<ElementId>, state: CredState, fraction: f32, refreshing: bool, diameter: f32, p: &Palette) -> gpui::AnyElement {
    if refreshing {
        Ring::refreshing(diameter, p)
            .with_animation(id, Animation::new(Duration::from_millis(1100)).repeat().with_max_fps(30.), |mut r, t| {
                r.spin = Some(t);
                r
            })
            .into_any_element()
    } else {
        Ring::new(state, fraction, diameter, p).into_any_element()
    }
}
