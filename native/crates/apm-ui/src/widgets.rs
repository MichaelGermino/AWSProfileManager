//! Small building blocks every view uses, so spacing, colour and accessibility stay consistent.
//! Every interactive element gets a role and an accessible name: the shared UI flows
//! (native/tests/fixtures/flows) find controls by exactly those, through UI Automation.

use gpui::{div, prelude::*, px, svg, Div, ElementId, Role, SharedString, Stateful, Svg};
use gpui_component::tooltip::Tooltip;

use crate::assets;
use crate::theme::{size, Palette, FONT_MONO};

pub fn icon(name: &str, size_px: f32, color: gpui::Rgba) -> Svg {
    svg().path(assets::icon(name)).size(px(size_px)).text_color(color).flex_shrink_0()
}

/// Square icon-only button with a tooltip; `label` is its accessible name.
pub fn icon_button(id: impl Into<ElementId>, name: &str, label: impl Into<SharedString>, p: &Palette) -> Stateful<Div> {
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
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(icon(name, 16., p.text_muted))
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
        .when_some(icon_name, |d, n| d.child(icon(n, 16., p.on_primary)))
        .child(label)
}

/// Bordered secondary button.
pub fn secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon_name: Option<&str>, p: &Palette) -> Stateful<Div> {
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
        .when_some(icon_name, |d, n| d.child(icon(n, 16., p.text)))
        .child(label)
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
