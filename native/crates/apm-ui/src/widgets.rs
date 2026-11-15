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

/// The status dot, breathing softly while its profile refreshes.
pub fn status_dot_busy(id: impl Into<ElementId>, color: Option<gpui::Rgba>, ring: gpui::Rgba, diameter: f32, busy: bool) -> gpui::AnyElement {
    let dot = status_dot(color, ring, diameter);
    if busy {
        crate::motion::pulse(dot, id).into_any_element()
    } else {
        dot.into_any_element()
    }
}

/// The status dot (signature detail). `None` colour = hollow ring (never signed in).
pub fn status_dot(color: Option<gpui::Rgba>, ring: gpui::Rgba, diameter: f32) -> Div {
    let d = div().size(px(diameter)).rounded_full().flex_shrink_0();
    match color {
        Some(c) => d.bg(c),
        None => d.border_1().border_color(ring),
    }
}

/// 1.x ProfileAvatar: the profile's icon in its colour on a small tile. Decorative (the name sits
/// beside it), so it stays out of the accessibility tree.
pub fn profile_avatar(icon_name: Option<&str>, icon_color: Option<&str>, tile: f32, glyph: f32, p: &Palette) -> Div {
    use crate::profile_icons as pi;
    let id = pi::effective_icon(icon_name);
    let color = pi::rgba(&pi::effective_color(icon_color));
    div()
        .size(px(tile))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(if tile >= 40. { 10. } else { 8. }))
        .border_1()
        .border_color(p.border)
        .bg(p.raised)
        .child(svg().path(pi::asset(id)).size(px(glyph)).text_color(color).flex_shrink_0())
}
