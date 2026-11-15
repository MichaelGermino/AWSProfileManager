//! The main window: client-drawn title bar, navigation rail, the current page, and toasts.

use gpui::{div, prelude::*, px, Context, Entity, FocusHandle, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::TitleBar;

use crate::a11y::PressExt as _;
use crate::dialogs::DialogView;
use crate::lock::LockView;
use crate::model::{AppModel, LockState, Page};
use crate::profiles::ProfilesView;
use crate::settings::SettingsView;
use crate::theme::{size, FONT_SANS};
use crate::widgets::icon;
use crate::wizard::WizardView;

pub struct Shell {
    model: Entity<AppModel>,
    profiles: Entity<ProfilesView>,
    /// The view for `model.dialog`, rebuilt when `dialog_seq` changes.
    dialog: Option<(u64, Entity<DialogView>)>,
    lock: Option<Entity<LockView>>,
    /// Built the first time Settings opens; refreshed each time it opens again (1.x reloaded on show).
    settings: Option<Entity<SettingsView>>,
    last_page: Page,
    /// The setup wizard, rebuilt when `wizard_seq` changes.
    wizard: Option<(u64, Entity<WizardView>)>,
    /// Keyboard focus for the tour card (Escape, arrows).
    tour_focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Shell {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profiles = cx.new(|cx| ProfilesView::new(model.clone(), window, cx));
        let subs = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            // Follow the OS light/dark switch live when the theme is "system".
            cx.observe_window_appearance(window, |this, window, cx| {
                let a = window.appearance();
                this.model.update(cx, |m, cx| m.set_os_appearance(a, cx));
            }),
        ];
        Shell {
            model,
            profiles,
            dialog: None,
            lock: None,
            settings: None,
            last_page: Page::Profiles,
            wizard: None,
            tour_focus: cx.focus_handle(),
            _subs: subs,
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (lock, dialog, seq, wizard_open, wizard_seq) = {
            let m = self.model.read(cx);
            (m.lock, m.dialog.clone(), m.dialog_seq, m.wizard_open, m.wizard_seq)
        };
        if wizard_open && lock == LockState::Unlocked {
            if self.wizard.as_ref().map(|(s, _)| *s) != Some(wizard_seq) {
                let model = self.model.clone();
                self.wizard = Some((wizard_seq, cx.new(|cx| WizardView::new(model, window, cx))));
            }
        } else {
            self.wizard = None;
        }
        // The dialog view follows the model's dialog.
        match dialog {
            Some(kind) if self.dialog.as_ref().map(|(s, _)| *s) != Some(seq) => {
                let model = self.model.clone();
                self.dialog = Some((seq, cx.new(|cx| DialogView::new(model, kind, window, cx))));
            }
            None => self.dialog = None,
            _ => {}
        }
        match lock {
            LockState::NeedsCreate | LockState::NeedsUnlock => {
                if self.lock.is_none() {
                    let model = self.model.clone();
                    self.lock = Some(cx.new(|cx| LockView::new(model, window, cx)));
                }
            }
            _ => self.lock = None,
        }
        let m = self.model.read(cx).snapshot();
        let p = m.palette;
        let page = m.page;
        if page == Page::Settings && self.last_page != Page::Settings {
            match &self.settings {
                Some(v) => v.update(cx, |v, cx| v.refresh(window, cx)),
                None => {
                    let model = self.model.clone();
                    self.settings = Some(cx.new(|cx| SettingsView::new(model, window, cx)));
                }
            }
        }
        self.last_page = page;

        let collapsed = self.model.read(cx).sidebar_collapsed;
        // Expanded (1.x default): icons and labels. Collapsed: the icon rail, names in tooltips.
        let nav_item = |id: &'static str, label: &'static str, icon_name: &'static str, target: Page, cx: &mut Context<Self>| {
            let active = page == target;
            div()
                .id(id)
                .role(Role::Link)
                .aria_label(label)
                .focusable()
                .tab_stop(true)
                .relative()
                .h(px(40.))
                .flex()
                .items_center()
                .rounded(px(size::RADIUS))
                .cursor_pointer()
                .when(active, |d| d.bg(p.raised))
                .hover(|d| d.bg(p.hover))
                .when(collapsed, |d| {
                    d.w(px(40.)).justify_center().tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
                })
                .when(!collapsed, |d| d.w_full().px(px(11.)).gap(px(12.)))
                .on_press(cx.listener(move |this, _, _, cx| {
                    this.model.update(cx, |m, cx| {
                        m.page = target;
                        cx.notify();
                    })
                }))
                .when(active, |d| d.child(div().absolute().left(px(-8.)).top(px(10.)).w(px(3.)).h(px(20.)).rounded(px(2.)).bg(p.accent)))
                .child(icon(icon_name, 18., if active { p.text } else { p.text_muted }))
                .when(!collapsed, |d| {
                    d.child(
                        div()
                            .text_size(px(size::TEXT))
                            .when(active, |t| t.font_weight(FontWeight::MEDIUM).text_color(p.text))
                            .when(!active, |t| t.text_color(p.text_muted))
                            .child(label),
                    )
                })
        };

        let toggle_label = if collapsed { "Expand sidebar" } else { "Collapse sidebar" };
        let toggle = crate::widgets::icon_button("sidebar-toggle", if collapsed { "panel-left-open" } else { "panel-left-close" }, toggle_label, &p)
            .size(px(40.))
            .on_press(cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.set_sidebar_collapsed(!collapsed, cx))));

        let rail = div()
            .id("main-nav")
            .role(Role::Navigation)
            .aria_label("Main")
            .w(px(if collapsed { size::RAIL } else { size::SIDEBAR }))
            .flex_shrink_0()
            .h_full()
            .bg(p.rail)
            .border_r_1()
            .border_color(p.border)
            .flex()
            .flex_col()
            .when(collapsed, |d| d.items_center())
            .when(!collapsed, |d| d.px(px(8.)))
            .py(px(8.))
            .gap(px(4.))
            .child(div().pb(px(4.)).child(toggle))
            .child(nav_item("nav-profiles", "Profiles", "users", Page::Profiles, cx))
            .child(nav_item("nav-terminal", "Terminal", "terminal", Page::Terminal, cx))
            .child(div().flex_1())
            .child(nav_item("nav-settings", "Settings", "sliders-horizontal", Page::Settings, cx));

        let content: gpui::AnyElement = if let Some(l) = &self.lock {
            l.clone().into_any_element()
        } else if lock == LockState::Loading {
            div().into_any_element()
        } else {
            match page {
            Page::Profiles => self.profiles.clone().into_any_element(),
            Page::Terminal => placeholder("Terminal", "The terminal comes in a later Phase 4 step.", &p).into_any_element(),
            Page::Settings => match &self.settings {
                Some(v) => v.clone().into_any_element(),
                None => div().into_any_element(),
            },
            }
        };
        let locked = lock != LockState::Unlocked;
        let (changelog, tour_offered, tour, paused_alert, bridge) = {
            let m = self.model.read(cx);
            (m.changelog.clone(), m.pending_tour.is_some(), m.tour.clone(), m.paused_alert, m.bridge.clone())
        };

        let title_bar = TitleBar::new().bg(p.rail).border_b_1().border_color(p.border).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(18.))
                        .rounded(px(5.))
                        .bg(p.primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("key-round", 12., p.on_primary)),
                )
                .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text).child("AWS Profile Manager")),
        );

        let mut root = div()
            .id("app-root")
            .role(Role::Application)
            .aria_label("AWS Profile Manager")
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.text)
            .font_family(FONT_SANS)
            .text_size(px(size::TEXT))
            .child(title_bar)
            .child(if locked {
                div().flex_1().min_h(px(0.)).child(content)
            } else {
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .flex()
                    .child(rail)
                    .child(div().flex_1().min_w(px(0.)).h_full().child(content))
                    // The wizard owns the whole window below the title bar, as in 1.x.
                    .when_some(self.wizard.as_ref().map(|(_, w)| w.clone()), |d, w| d.child(w))
            });
        if let Some((_, d)) = &self.dialog {
            root = root.child(d.clone());
        }
        if !locked {
            if let Some(t) = &tour {
                if !self.tour_focus.contains_focused(window, cx) {
                    window.focus(&self.tour_focus, cx);
                }
                root = root.child(crate::onboarding::tour_card(&self.model, t, &self.tour_focus, &p));
            } else if let (Some(c), None) = (&changelog, &self.wizard) {
                root = root.child(crate::onboarding::whats_new(&self.model, bridge, c, tour_offered, &p, window));
            }
            if paused_alert {
                root = root.child(crate::onboarding::paused_alert(&self.model, &p));
            }
        }

        if let Some(t) = m.toast.clone() {
            root = root.child(
                div()
                    .id("toast")
                    .absolute()
                    .bottom(px(16.))
                    .right(px(16.))
                    .w(px(360.))
                    .p(px(14.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_color(if t.error { p.danger } else { p.border_strong })
                    .bg(p.panel)
                    .role(Role::Alert)
                    .aria_label(SharedString::from(format!("{}: {}", t.title, t.body)))
                    .child(div().font_weight(FontWeight::MEDIUM).child(t.title))
                    .child(div().mt(px(2.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(t.body)),
            );
        }
        root
    }
}

fn placeholder(title: &'static str, body: &'static str, p: &crate::theme::Palette) -> impl IntoElement {
    div()
        .size_full()
        .p(px(24.))
        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().mt(px(6.)).text_color(p.text_muted).child(body))
}
