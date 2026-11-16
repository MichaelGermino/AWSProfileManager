//! Small building blocks every view uses, so spacing, colour and accessibility stay consistent.
//! Every interactive element gets a role and an accessible name: the shared UI flows
//! (native/tests/fixtures/flows) find controls by exactly those, through UI Automation.

use gpui::{div, prelude::*, px, svg, Div, ElementId, InteractiveElement, Role, SharedString, Stateful, Styled, Svg};
use gpui_component::tooltip::Tooltip;

use crate::assets;
use crate::theme::{size, Palette, FONT_MONO};

pub fn icon(name: &str, size_px: f32, color: gpui::Rgba) -> Svg {
    svg().path(assets::icon(name)).size(px(size_px)).text_color(color).flex_shrink_0()
}

/// Square icon-only button with a tooltip; `label` is its accessible name.
pub fn icon_button(id: impl Into<ElementId>, name: &str, label: impl Into<SharedString>, p: &Palette) -> Stateful<Div> {
    icon_button_with(id, icon(name, 16., p.text_muted), label, p)
}

/// The refresh button: its arrows turn while the refresh runs.
pub fn refresh_button(id: impl Into<ElementId>, label: impl Into<SharedString>, busy: bool, p: &Palette) -> Stateful<Div> {
    let id: ElementId = id.into();
    let glyph = icon("refresh-cw", 16., p.text_muted);
    if busy {
        let spin_id = SharedString::from(format!("{id}-spin"));
        icon_button_with(id, crate::motion::spin(glyph, spin_id), label, p)
    } else {
        icon_button_with(id, glyph, label, p)
    }
}

/// [`icon_button`] around any glyph.
pub fn icon_button_with(id: impl Into<ElementId>, glyph: impl IntoElement, label: impl Into<SharedString>, p: &Palette) -> Stateful<Div> {
    let label: SharedString = label.into();
    let tip = label.clone();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .focusable()
        .tab_stop(true)
        .size(px(size::CONTROL))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .cursor_pointer()
        .text_color(p.text_muted)
        .hover(|s| s.bg(p.hover).text_color(p.text))
        .active(|s| s.bg(p.raised))
        .focus_ring(p)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(glyph)
}

/// The one strong button on a screen.
pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon_name: Option<&str>, p: &Palette) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_stop(true)
        .h(px(size::BUTTON))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(size::RADIUS))
        .bg(p.primary)
        .text_color(p.on_primary)
        .text_size(px(size::TEXT))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.84))
        .focus_visible({
            let ring = p.focus_ring(true);
            move |s| s.shadow(ring)
        })
        .when_some(icon_name, |d, n| d.child(icon(n, 16., p.on_primary)))
        .child(label)
}

/// Bordered secondary button.
pub fn secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon_name: Option<&str>, p: &Palette) -> Stateful<Div> {
    secondary_button_with(id, label, icon_name.map(|n| icon(n, 16., p.text).into_any_element()), p)
}

/// A secondary "Refresh" button whose arrows turn while the refresh runs.
pub fn refresh_secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, busy: bool, p: &Palette) -> Stateful<Div> {
    let id: ElementId = id.into();
    let glyph = icon("refresh-cw", 16., p.text);
    let glyph = if busy { crate::motion::spin(glyph, SharedString::from(format!("{id}-spin"))).into_any_element() } else { glyph.into_any_element() };
    secondary_button_with(id, label, Some(glyph), p)
}

/// [`secondary_button`] with any leading glyph.
pub fn secondary_button_with(id: impl Into<ElementId>, label: impl Into<SharedString>, glyph: Option<gpui::AnyElement>, p: &Palette) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_stop(true)
        .h(px(size::BUTTON))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(size::RADIUS))
        .border_1()
        .border_color(p.border)
        .bg(p.raised)
        .text_color(p.text)
        .text_size(px(size::TEXT))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.bg(p.hover))
        .active(|s| s.bg(p.raised).border_color(p.border_strong))
        .focus_ring(p)
        .children(glyph)
        .child(label)
}

/// The keyboard focus ring (palette accent) on a control we draw ourselves. Shown only for keyboard
/// focus (GPUI's focus-visible), drawn inside the control so it never moves anything.
pub trait FocusRing: InteractiveElement + Sized {
    fn focus_ring(self, p: &Palette) -> Self {
        let ring = p.focus_ring(false);
        self.focus_visible(move |s| s.shadow(ring))
    }
}

impl<E: InteractiveElement> FocusRing for E {}

/// A segment of a segmented control or a view toggle: chosen ones sit raised out of the track.
pub fn chip<E: Styled>(el: E, on: bool, p: &Palette) -> E {
    if on {
        el.bg(p.chip()).shadow(p.chip_shadow())
    } else {
        el
    }
}

pub fn mono(text: impl Into<SharedString>, color: gpui::Rgba) -> Div {
    div().font_family(FONT_MONO).text_size(px(size::TEXT_XS)).text_color(color).child(text.into())
}

/// Text that assistive technology should read (GPUI leaves plain text out of the accessibility
/// tree). A Label's accessible name comes from its value, so both are set.
pub fn read_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text: SharedString = text.into();
    div().id(id).role(Role::Label).aria_label(text.clone()).aria_value(text.clone()).child(text)
}

/// The status dot (signature detail). `None` colour = hollow ring (never signed in).
pub fn status_dot(color: Option<gpui::Rgba>, ring: gpui::Rgba, diameter: f32) -> Div {
    let d = div().size(px(diameter)).rounded_full().flex_shrink_0();
    match color {
        Some(c) => d.bg(c),
        None => d.border_1().border_color(ring),
    }
}

/// A profile's avatar tile: its picked icon in its colour, or (no icon picked) its initials on a
/// tile tinted with its colour (`profile_icons::Avatar`). Decorative (the name sits beside it), so
/// it stays out of the accessibility tree.
pub fn profile_avatar(avatar: &crate::profile_icons::Avatar, tile: f32, glyph: f32, p: &Palette) -> Div {
    use crate::profile_icons::{self as pi, Avatar};
    let base = avatar.color();
    let alpha = pi::avatar_tint_alpha(p.is_dark());
    let d = div()
        .size(px(tile))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(if tile >= 40. { 10. } else { 8. }))
        .bg(gpui::Rgba { a: alpha, ..base });
    match avatar {
        Avatar::Icon { icon, color } => d.child(svg().path(pi::asset(icon)).size(px(glyph)).text_color(pi::rgba(color)).flex_shrink_0()),
        Avatar::Initials { text, light, dark, .. } => d
            .text_size(px((tile * 0.4).round()))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(gpui::rgb(if p.is_dark() { *dark } else { *light }))
            .line_height(px(tile))
            .child(text.clone()),
    }
}

/// The avatar of a profile record.
pub fn avatar_of(prof: &serde_json::Value) -> crate::profile_icons::Avatar {
    let s = |k: &str| prof.get(k).and_then(serde_json::Value::as_str);
    crate::profile_icons::Avatar::of(s("iconName"), s("iconColor"), s("name").unwrap_or(""), crate::profile_icons::account_of(prof).as_deref())
}

/// The page colour with a trace of the accent: the backdrop of the lock screen and the splash, so
/// the app's own screens read as this app before any profile shows.
pub fn identity_bg(p: &Palette) -> gpui::Rgba {
    let t = if p.is_dark() { 0.045 } else { 0.035 };
    let mix = |a: f32, b: f32| a + (b - a) * t;
    gpui::Rgba { r: mix(p.bg.r, p.accent.r), g: mix(p.bg.g, p.accent.g), b: mix(p.bg.b, p.accent.b), a: 1. }
}

/// A quiet field of accent dots on a regular grid over [`identity_bg`], filling its parent (which
/// must be `relative`). Static: painted once per frame the parent paints, never on its own.
pub fn identity_field(p: &Palette) -> impl IntoElement {
    let dot = gpui::Rgba { a: if p.is_dark() { 0.2 } else { 0.18 }, ..p.accent };
    gpui::canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            const STEP: f32 = 24.;
            const DOT: f32 = 2.;
            let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
            // Centred, so the margins match on every side.
            let ox = x0 + (w % STEP) / 2. + STEP / 2.;
            let oy = y0 + (h % STEP) / 2. + STEP / 2.;
            let mut y = oy;
            while y < y0 + h {
                let mut x = ox;
                while x < x0 + w {
                    let at = gpui::Bounds::centered_at(gpui::point(px(x), px(y)), gpui::size(px(DOT), px(DOT)));
                    window.paint_quad(gpui::fill(at, dot).corner_radii(px(DOT / 2.)));
                    x += STEP;
                }
                y += STEP;
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
