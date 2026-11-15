//! A small popup menu: items, separators and expandable groups ("Move to folder", "Change color").
//!
//! Our own rather than gpui-component's `PopupMenu`: its items always carry a selected state, and
//! accesskit then offers UI Automation no Invoke pattern for them, so neither assistive tools nor
//! the UI flow runner could activate an item without a real mouse. Here every item is a plain
//! clickable element (Invoke), and a group exposes Expand / Collapse.

use std::collections::HashSet;
use std::rc::Rc;

use gpui::{div, prelude::*, px, App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, IntoElement, Role, SharedString, Window};

use crate::a11y::PressExt as _;
use crate::a11y::A11yDisabledExt;
use crate::theme::{size, Palette};
use crate::widgets::icon;

pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum Entry {
    Item { label: SharedString, swatch: Option<gpui::Rgba>, checked: bool, danger: bool, disabled: bool, tag: Option<SharedString>, on_click: Handler },
    Separator,
    Group { label: SharedString, children: Vec<Entry> },
    /// A grid of colour swatches; each is a menu item named by its label.
    Swatches(Vec<Swatch>),
}

pub struct Swatch {
    pub label: SharedString,
    pub color: gpui::Rgba,
    pub checked: bool,
    pub on_click: Handler,
}

impl Entry {
    pub fn item(label: impl Into<SharedString>, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Entry::Item { label: label.into(), swatch: None, checked: false, danger: false, disabled: false, tag: None, on_click: Rc::new(on_click) }
    }

    pub fn checked(mut self, on: bool) -> Self {
        if let Entry::Item { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    /// Shown but not choosable, with a short muted note after the label ("current").
    pub fn disabled_with(mut self, note: impl Into<SharedString>) -> Self {
        if let Entry::Item { disabled, tag, .. } = &mut self {
            *disabled = true;
            *tag = Some(note.into());
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let Entry::Item { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }

    pub fn swatch(mut self, color: gpui::Rgba) -> Self {
        if let Entry::Item { swatch, .. } = &mut self {
            *swatch = Some(color);
        }
        self
    }
}

pub struct MenuView {
    entries: Vec<Entry>,
    expanded: HashSet<usize>,
    palette: Palette,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for MenuView {}

impl Focusable for MenuView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MenuView {
    pub fn new(entries: Vec<Entry>, palette: Palette, cx: &mut Context<Self>) -> Self {
        MenuView { entries, expanded: HashSet::new(), palette, focus: cx.focus_handle() }
    }

    /// `None` toggles.
    fn set_expanded(&mut self, key: usize, open: Option<bool>, cx: &mut Context<Self>) {
        let now = self.expanded.contains(&key);
        if open.unwrap_or(!now) {
            self.expanded.insert(key);
        } else {
            self.expanded.remove(&key);
        }
        cx.notify();
    }

    fn render_entries(&self, entries: &[Entry], path: usize, depth: usize, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = self.palette;
        let me = cx.entity().downgrade();
        let mut out = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let key = path * 100 + i;
            match e {
                Entry::Separator => out.push(div().my(px(4.)).h(px(1.)).bg(p.border).into_any_element()),
                Entry::Item { label, swatch, checked, danger, disabled, tag, on_click } => {
                    let on_click = on_click.clone();
                    let disabled = *disabled;
                    let name: SharedString = match tag {
                        Some(t) => format!("{label} {t}").into(),
                        None => label.clone(),
                    };
                    out.push(
                        div()
                            .id(SharedString::from(format!("menu-item-{key}")))
                            .role(Role::MenuItem)
                            .aria_label(name)
                            .h(px(30.))
                            .pl(px(10. + depth as f32 * 14.))
                            .pr(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(6.))
                            .text_size(px(size::TEXT_SM))
                            .text_color(if disabled { p.text_faint } else if *danger { p.danger } else { p.text })
                            .when(!disabled, |d| {
                                d.cursor_pointer().hover(|d| d.bg(p.hover)).on_press(cx.listener(move |_, _, window, cx| {
                                    cx.emit(DismissEvent);
                                    on_click(window, cx);
                                }))
                            })
                            .when_some(*swatch, |d, c| d.child(div().size(px(12.)).rounded(px(3.)).bg(c)))
                            .child(div().flex_1().child(label.clone()))
                            .when_some(tag.clone(), |d, t| d.child(div().text_size(px(size::TEXT_XS)).text_color(p.text_faint).child(t)))
                            .when(*checked, |d| d.child(icon("check", 14., p.text_muted)))
                            .a11y_disabled(disabled)
                            .into_any_element(),
                    );
                }
                Entry::Swatches(items) => {
                    let mut grid = div().pl(px(10. + depth as f32 * 14.)).pr(px(10.)).py(px(6.)).flex().flex_wrap().gap(px(6.)).w(px(196.));
                    for (j, sw) in items.iter().enumerate() {
                        let on_click = sw.on_click.clone();
                        grid = grid.child(
                            div()
                                .id(SharedString::from(format!("menu-swatch-{key}-{j}")))
                                .role(Role::MenuItem)
                                .aria_label(sw.label.clone())
                                .size(px(24.))
                                .rounded(px(6.))
                                .bg(sw.color)
                                .border_2()
                                .border_color(if sw.checked { p.text } else { gpui::Rgba { a: 0., ..sw.color } })
                                .cursor_pointer()
                                .hover(|d| d.border_color(p.text_muted))
                                .tooltip({
                                    let tip = sw.label.clone();
                                    move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                                })
                                .on_press(cx.listener(move |_, _, window, cx| {
                                    cx.emit(DismissEvent);
                                    on_click(window, cx);
                                })),
                        );
                    }
                    out.push(grid.into_any_element());
                }
                Entry::Group { label, children } => {
                    let open = self.expanded.contains(&key);
                    out.push(
                        div()
                            .id(SharedString::from(format!("menu-group-{key}")))
                            .role(Role::MenuItem)
                            .aria_label(label.clone())
                            .aria_expanded(open)
                            .h(px(30.))
                            .pl(px(10. + depth as f32 * 14.))
                            .pr(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(6.))
                            .text_size(px(size::TEXT_SM))
                            .text_color(p.text)
                            .cursor_pointer()
                            .hover(|d| d.bg(p.hover))
                            .on_press(cx.listener(move |this, _, _, cx| this.set_expanded(key, None, cx)))
                            // UI Automation's Expand / Collapse arrive as their own actions.
                            .on_a11y_action(gpui::AccessibleAction::Expand, {
                                let me = me.clone();
                                move |_, _, cx| {
                                    let _ = me.update(cx, |this, cx| this.set_expanded(key, Some(true), cx));
                                }
                            })
                            .on_a11y_action(gpui::AccessibleAction::Collapse, {
                                let me = me.clone();
                                move |_, _, cx| {
                                    let _ = me.update(cx, |this, cx| this.set_expanded(key, Some(false), cx));
                                }
                            })
                            .child(div().flex_1().child(label.clone()))
                            .child(icon(if open { "chevron-down" } else { "chevron-right" }, 14., p.text_faint))
                            .into_any_element(),
                    );
                    if open {
                        out.extend(self.render_entries(children, key + 1, depth + 1, cx));
                    }
                }
            }
        }
        out
    }
}

impl Render for MenuView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let entries = std::mem::take(&mut self.entries);
        let children = self.render_entries(&entries, 0, 0, cx);
        self.entries = entries;
        div()
            .id("popup-menu")
            .role(Role::Menu)
            .track_focus(&self.focus)
            .min_w(px(200.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.panel)
            .shadow_lg()
            .on_key_down(cx.listener(|_, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    cx.emit(DismissEvent);
                }
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            .children(children)
    }
}
