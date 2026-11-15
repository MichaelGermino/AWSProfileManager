//! Disabled semantics for our own controls. GPUI has no `aria_disabled` setter; this wraps a
//! control's root element and marks its accessibility node disabled, so a disabled button reads as
//! disabled to assistive technology and to the UI flow runner.
//!
//! Adapted from gpui-base 0.9.1 `a11y_disabled.rs` (GPUI Kit, Apache-2.0), where it is crate-private.

use gpui::{
    accesskit, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, InteractiveElement, Interactivity,
    IntoElement, LayoutId, Pixels, Window,
};

pub struct A11yDisabled<E> {
    inner: E,
    disabled: bool,
}

pub trait A11yDisabledExt: Sized {
    fn a11y_disabled(self, disabled: bool) -> A11yDisabled<Self>;
}

impl<E: Element + InteractiveElement> A11yDisabledExt for E {
    fn a11y_disabled(self, disabled: bool) -> A11yDisabled<Self> {
        A11yDisabled { inner: self, disabled }
    }
}

impl<E: Element> IntoElement for A11yDisabled<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: InteractiveElement> InteractiveElement for A11yDisabled<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl<E: Element> Element for A11yDisabled<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner.prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(id, inspector, bounds, layout, prepaint, window, cx)
    }

    fn a11y_role(&self) -> Option<gpui::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }

    fn a11y_synthetic_children(&mut self, prepaint: &mut Self::PrepaintState, builder: &mut gpui::A11ySubtreeBuilder) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}
