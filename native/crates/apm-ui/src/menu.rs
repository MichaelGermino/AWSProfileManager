//! A small popup menu: items, separators and expandable groups ("Move to folder", "Change color").
//!
//! Our own rather than gpui-component's `PopupMenu`: its items always carry a selected state, and
//! accesskit then offers UI Automation no Invoke pattern for them, so neither assistive tools nor
//! the UI flow runner could activate an item without a real mouse. Here every item is a plain
//! clickable element (Invoke), and a group exposes Expand / Collapse.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{div, prelude::*, px, App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, IntoElement, Role, SharedString, Window};

use crate::a11y::A11yDisabledExt;
use crate::a11y::PressExt as _;
use crate::theme::{size, Palette};
use crate::widgets::{icon, FocusRing as _};
use gpui_component::FocusTrapElement as _;

pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum Entry {
    Item {
        label: SharedString,
        swatch: Option<gpui::Rgba>,
        checked: bool,
        danger: bool,
        disabled: bool,
        tag: Option<SharedString>,
        /// A muted second line under the label (not part of the accessible name).
        description: Option<SharedString>,
        on_click: Handler,
    },
    Separator,
    Group {
        label: SharedString,
        children: Vec<Entry>,
    },
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
        Entry::Item {
            label: label.into(),
            swatch: None,
            checked: false,
            danger: false,
            disabled: false,
            tag: None,
            description: None,
            on_click: Rc::new(on_click),
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        if let Entry::Item { description, .. } = &mut self {
            *description = Some(text.into());
        }
        self
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
    /// One focus handle per item, so arrow keys and Tab move through them.
    item_focus: HashMap<String, FocusHandle>,
    /// The items in the order they were drawn (for the arrow keys).
    order: Vec<FocusHandle>,
}

impl EventEmitter<DismissEvent> for MenuView {}

impl Focusable for MenuView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MenuView {
    pub fn new(entries: Vec<Entry>, palette: Palette, cx: &mut Context<Self>) -> Self {
        MenuView { entries, expanded: HashSet::new(), palette, focus: cx.focus_handle(), item_focus: HashMap::new(), order: Vec::new() }
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

    /// The focus handle for the item with this element id (made the first time it is drawn).
    fn item_handle(&mut self, key: &str, cx: &mut Context<Self>) -> FocusHandle {
        let h = self.item_focus.entry(key.to_string()).or_insert_with(|| cx.focus_handle().tab_stop(true)).clone();
        self.order.push(h.clone());
        h
    }

    /// Up / Down (and Home / End) move between items, wrapping around.
    fn move_focus(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.order.is_empty() {
            return;
        }
        let n = self.order.len();
        let at = self.order.iter().position(|h| h.is_focused(window));
        let next = match (key, at) {
            ("home", _) => 0,
            ("end", _) => n - 1,
            ("down", Some(i)) => (i + 1) % n,
            ("down", None) => 0,
            ("up", Some(i)) => (i + n - 1) % n,
            ("up", None) => n - 1,
            _ => return,
        };
        window.focus(&self.order[next].clone(), cx);
    }

    fn render_entries(&mut self, entries: &[Entry], path: usize, depth: usize, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = self.palette;
        let me = cx.entity().downgrade();
        let mut out = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let key = path * 100 + i;
            match e {
                Entry::Separator => out.push(div().my(px(4.)).h(px(1.)).bg(p.border).into_any_element()),
                Entry::Item { label, swatch, checked, danger, disabled, tag, description, on_click } => {
                    let on_click = on_click.clone();
                    let disabled = *disabled;
                    let name: SharedString = match tag {
                        Some(t) => format!("{label} {t}").into(),
                        None => label.clone(),
                    };
                    let eid = format!("menu-item-{key}");
                    let handle = if disabled { None } else { Some(self.item_handle(&eid, cx)) };
                    out.push(
                        div()
                            .id(SharedString::from(eid))
                            .role(Role::MenuItem)
                            .aria_label(name)
                            .when_some(handle, |d, h| d.track_focus(&h).focus_ring(&p))
                            .when(description.is_none(), |d| d.h(px(30.)))
                            .when(description.is_some(), |d| d.py(px(8.)))
                            .pl(px(10. + depth as f32 * 14.))
                            .pr(px(10.))
                            .flex()
                            .when(description.is_none(), |d| d.items_center())
                            .gap(px(8.))
                            .rounded(px(6.))
                            .text_size(px(size::TEXT_SM))
                            .text_color(if disabled {
                                p.text_faint
                            } else if *danger {
                                p.danger
                            } else {
                                p.text
                            })
                            .when(!disabled, |d| {
                                d.cursor_pointer().hover(|d| d.bg(p.hover)).on_press(cx.listener(move |_, _, window, cx| {
                                    cx.emit(DismissEvent);
                                    on_click(window, cx);
                                }))
                            })
                            .when_some(*swatch, |d, c| d.child(div().size(px(12.)).rounded(px(3.)).bg(c)))
                            .child(match description {
                                Some(text) => div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(div().font_weight(gpui::FontWeight::MEDIUM).child(label.clone()))
                                    .child(div().text_color(p.text_muted).child(text.clone())),
                                None => div().flex_1().child(label.clone()),
                            })
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
                        let eid = format!("menu-swatch-{key}-{j}");
                        let handle = self.item_handle(&eid, cx);
                        grid = grid.child(
                            div()
                                .id(SharedString::from(eid))
                                .role(Role::MenuItem)
                                .track_focus(&handle)
                                .focus_visible({
                                    let ring = p.text;
                                    move |d| d.border_color(ring)
                                })
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
                    let eid = format!("menu-group-{key}");
                    let handle = self.item_handle(&eid, cx);
                    out.push(
                        div()
                            .id(SharedString::from(eid))
                            .track_focus(&handle)
                            .focus_ring(&p)
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
                            // Right opens a group, Left closes it (as in a native menu).
                            .on_key_down(cx.listener(move |this, ev: &gpui::KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                                "right" => this.set_expanded(key, Some(true), cx),
                                "left" => this.set_expanded(key, Some(false), cx),
                                _ => {}
                            }))
                            .child(crate::motion::chevron(icon("chevron-right", 14., p.text_faint), SharedString::from(format!("menu-chevron-{key}")), open))
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
        self.order.clear();
        let children = self.render_entries(&entries, 0, 0, cx);
        self.entries = entries;
        let menu = div()
            .id("popup-menu")
            .role(Role::Menu)
            .min_w(px(200.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.panel)
            .shadow_lg()
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| match ev.keystroke.key.as_str() {
                "escape" => cx.emit(DismissEvent),
                k @ ("up" | "down" | "home" | "end") => {
                    cx.stop_propagation();
                    this.move_focus(k, window, cx)
                }
                _ => {}
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            .children(children)
            // Tab cycles through the items instead of leaving the menu.
            .focus_trap("menu-trap", &self.focus);
        crate::motion::popover_in(menu, "menu-in")
    }
}
