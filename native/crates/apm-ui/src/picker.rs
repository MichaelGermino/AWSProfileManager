//! A select box: a combobox trigger and a list of options under it.
//!
//! Our own rather than gpui-component's `Select`: that one reports an expanded state, so UI
//! Automation offers Expand / Collapse for it, but it only reacts to a click; assistive tools (and
//! the UI flow runner) could not open it. Here the trigger answers Click, Expand and Collapse, and
//! each option is a list item with Invoke.

use gpui::{
    anchored, deferred, div, prelude::*, px, AccessibleAction, App, Context, ElementId, EventEmitter, FocusHandle, Focusable,
    IntoElement, Role, SharedString, Window,
};

use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::icon;

#[derive(Clone)]
pub struct PickerOption {
    pub value: String,
    pub label: SharedString,
}

pub enum PickerEvent {
    Changed(Option<String>),
}

pub struct Picker {
    id: ElementId,
    label: SharedString,
    placeholder: SharedString,
    options: Vec<PickerOption>,
    selected: Option<String>,
    open: bool,
    mono: bool,
    focus: FocusHandle,
}

impl EventEmitter<PickerEvent> for Picker {}

impl Focusable for Picker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Picker {
    /// `label` is the accessible name (the field's label).
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Picker {
            id: id.into(),
            label: label.into(),
            placeholder: SharedString::default(),
            options: Vec::new(),
            selected: None,
            open: false,
            mono: false,
            focus: cx.focus_handle(),
        }
    }

    pub fn set_placeholder(&mut self, p: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = p.into();
        cx.notify();
    }

    pub fn set_options(&mut self, options: Vec<PickerOption>, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    /// Set without emitting a change (loading saved state).
    pub fn set_selected(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        self.selected = value;
        cx.notify();
    }

    pub fn selected(&self) -> Option<&String> {
        self.selected.as_ref()
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn choose(&mut self, value: String, cx: &mut Context<Self>) {
        self.open = false;
        self.selected = Some(value.clone());
        cx.emit(PickerEvent::Changed(Some(value)));
        cx.notify();
    }
}

/// The palette lives in the app model; the picker reads it from a global the shell keeps current.
pub struct PickerPalette(pub Palette);
impl gpui::Global for PickerPalette {}

impl Render for Picker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.try_global::<PickerPalette>().map(|g| g.0).unwrap_or_else(|| Palette::new(crate::theme::Style::Modern, crate::theme::Mode::Dark));
        let shown = self
            .selected
            .as_ref()
            .and_then(|v| self.options.iter().find(|o| &o.value == v))
            .map(|o| o.label.clone());
        let me = cx.entity().downgrade();
        let open = self.open;
        let trigger = div()
            .id(self.id.clone())
            .role(Role::ComboBox)
            .aria_label(self.label.clone())
            .aria_expanded(open)
            .when_some(shown.clone(), |d, s| d.aria_value(s))
            .track_focus(&self.focus)
            .h(px(size::CONTROL))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(if open { p.accent } else { p.border })
            .bg(p.sunken)
            .cursor_pointer()
            .text_size(px(size::TEXT_SM))
            .on_click(cx.listener(|this, _, window, cx| this.set_open(!this.open, window, cx)))
            .on_a11y_action(AccessibleAction::Expand, {
                let me = me.clone();
                move |_, window, cx| {
                    let _ = me.update(cx, |this, cx| this.set_open(true, window, cx));
                }
            })
            .on_a11y_action(AccessibleAction::Collapse, {
                let me = me.clone();
                move |_, window, cx| {
                    let _ = me.update(cx, |this, cx| this.set_open(false, window, cx));
                }
            })
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" && this.open {
                    this.set_open(false, window, cx);
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .when(self.mono, |d| d.font_family(FONT_MONO))
                    .text_color(if shown.is_some() { p.text } else { p.text_faint })
                    .child(shown.unwrap_or_else(|| self.placeholder.clone())),
            )
            .child(icon("chevron-down", 14., p.text_muted));

        let mut root = div().flex().flex_col().child(trigger);
        if open {
            let mut list = div()
                .id(SharedString::from(format!("{:?}-list", self.id)))
                .role(Role::ListBox)
                .aria_label(self.label.clone())
                .mt(px(4.))
                .min_w(px(280.))
                .max_h(px(320.))
                .overflow_y_scroll()
                .p(px(4.))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(p.border_strong)
                .bg(p.panel)
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| this.set_open(false, window, cx)));
            if self.options.is_empty() {
                list = list.child(div().px(px(10.)).py(px(8.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child("Nothing to choose yet"));
            }
            for (i, o) in self.options.iter().enumerate() {
                let value = o.value.clone();
                let checked = self.selected.as_deref() == Some(o.value.as_str());
                list = list.child(
                    div()
                        .id(SharedString::from(format!("opt-{i}")))
                        .role(Role::ListBoxOption)
                        .aria_label(o.label.clone())
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_size(px(size::TEXT_SM))
                        .cursor_pointer()
                        .hover(|d| d.bg(p.hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(value.clone(), cx)))
                        .child(div().flex_1().truncate().when(self.mono, |d| d.font_family(FONT_MONO)).child(o.label.clone()))
                        .when(checked, |d| d.child(icon("check", 14., p.text_muted))),
                );
            }
            root = root.child(deferred(anchored().snap_to_window().child(list)).with_priority(1));
        }
        root
    }
}
