//! What the app shows once after an update or on a first run (1.x `App.tsx` with
//! `ChangelogModal`, `tour/FeatureTour` and `tour/tourSteps`), plus the "Auto-refresh paused" alert
//! (1.x `Profiles.tsx`). The setup wizard itself is in `wizard.rs`.
//!
//! The core only keeps the bookkeeping (`changelog:getPending` / `markSeen`, `tour:getState` /
//! `markSeen`); which tour is due and its steps live here, as they lived in the 1.x renderer.
//!
//! 2.0 adaptation of the tour: 1.x cut a spotlight hole around the control each step talks about.
//! 2.0 switches to the screen (and Settings tab) the step is about and shows the step card at the
//! bottom of the window instead; the steps, their order and their copy are 1.x's, with wording
//! changed only where 2.0 moved a control (noted per step).

use gpui::{div, prelude::*, px, Entity, FocusHandle, FontWeight, IntoElement, KeyDownEvent, Role, SharedString, Window};
use gpui_component::text::{TextView, TextViewStyle};
use serde_json::{json, Value};

use crate::a11y::PressExt as _;
use crate::model::{AppModel, Page};
use crate::settings::Tab;
use crate::theme::{size, Palette};
use crate::widgets::{icon_button, primary_button, secondary_button, FocusRing as _};

/// Release notes for the running version (`changelog:getPending`).
#[derive(Debug, Clone)]
pub struct Changelog {
    pub version: String,
    pub notes: String,
    pub url: String,
}

/// The tour on screen.
#[derive(Debug, Clone)]
pub struct ActiveTour {
    /// Index into `TOURS`.
    pub tour: usize,
    /// The steps this user sees (indices into the tour's steps): steps whose control does not
    /// exist for them are left out up front, so the counter never jumps (1.x `skipIfMissing`).
    pub steps: Vec<usize>,
    /// Position in `steps`.
    pub index: usize,
    /// Where the user was when it started; the tour puts them back there.
    pub start_page: Page,
}

/// When a step can be shown.
#[derive(Clone, Copy)]
pub enum Needs {
    Always,
    /// The control only exists once there is at least one profile.
    Profiles,
}

pub struct TourStep {
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub page: Option<Page>,
    pub settings_tab: Option<Tab>,
    pub needs: Needs,
}

pub struct Tour {
    /// The only thing that decides who has seen a tour (`settings.seenTourIds`); never change it casually.
    pub id: &'static str,
    pub version: &'static str,
    pub steps: &'static [TourStep],
}

const fn step(id: &'static str, title: &'static str, body: &'static str, page: Option<Page>, settings_tab: Option<Tab>, needs: Needs) -> TourStep {
    TourStep { id, title, body, page, settings_tab, needs }
}

const DONE_BODY: &str = "The full release notes are on the GitHub release, linked from the What’s new popup.";

/// The "what's new after an update" sequence, oldest first (1.x `TOURS`).
pub const TOURS: &[Tour] = &[
    Tour {
        id: "v1.5-features-ga",
        version: "1.5.0",
        steps: &[
            step("welcome", "What's new in this version", "A few things worth two minutes — where the newest features live and what they do. You can leave at any time with Escape.", None, None, Needs::Always),
            step("bulk-import", "Import accounts in bulk", "Add accounts… signs you in once and then lets you pick which accounts and permission sets become profiles — instead of adding them one at a time. It works for both SAML and IAM Identity Center.", Some(Page::Profiles), None, Needs::Always),
            // 1.x: "right-click any profile"; 2.0 has the same item in the row's More actions menu.
            step("folders", "Group profiles into folders", "Make a folder with the New folder button, then drag profiles in — or open any profile's More actions menu and choose Move to folder, which is far easier across a long list. Folders are for your eyes only: they never change a profile name or how credentials are written.", Some(Page::Profiles), None, Needs::Profiles),
            step("one-click-console", "Open the AWS console in one click", "The Console button on a profile opens the AWS console already signed in as that profile — no copying credentials, no sign-in page. Expired credentials are refreshed for you first, and several accounts can be signed in at once. The same thing is in the tray menu under AWS Console.", Some(Page::Profiles), None, Needs::Profiles),
            step("background", "Give the app a background", "Pick any MP4 or WebM and choose where it plays — the sign-in screen only, or behind the whole window with translucent panels over it. The blur slider trades motion for legibility.", Some(Page::Settings), Some(Tab::Appearance), Needs::Always),
            step("done", "That's it", DONE_BODY, None, None, Needs::Always),
        ],
    },
    Tour {
        id: "v1.6-features",
        version: "1.6.0",
        steps: &[
            step("welcome", "What's new in this version", "A quick look at what changed: new app icons, a tray icon that tells you when credentials need attention, and easier profile editing. You can leave at any time with Escape.", None, None, Needs::Always),
            step("app-icon", "Pick a new app icon", "Choose from twenty designs. The window, taskbar, Dock and tray icon change straight away, no restart needed. Classic is the icon you have today, and it stays the default until you pick another.", Some(Page::Settings), Some(Tab::Appearance), Needs::Always),
            step("tray-status", "See credential problems from the tray", "A dot on the tray icon (the menu bar on macOS) shows amber when credentials are about to expire, red when auto refresh stopped after failures, and gray when it is paused. Monochrome draws just the symbol, to match the system icons next to it.", Some(Page::Settings), Some(Tab::Appearance), Needs::Always),
            step("edit-profile", "Editing a profile is quicker", "Save and Cancel are now at the top of the edit form as well as the bottom, so there is no scrolling to finish. Leaving the Profiles screen mid-edit cancels it: when you come back you see your list, and nothing was saved.", Some(Page::Profiles), None, Needs::Profiles),
            step("done", "That's it", DONE_BODY, None, None, Needs::Always),
        ],
    },
];

fn latest_tour() -> usize {
    TOURS.len() - 1
}

fn str_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

impl AppModel {
    /// After the app is unlocked, once per launch: the first-run wizard, the tour that is due and
    /// the release notes (1.x App.tsx effects gated on `masterPasswordState === 'unlocked'`).
    pub fn start_onboarding(&mut self, cx: &mut gpui::Context<Self>) {
        if self.onboarding_started {
            return;
        }
        self.onboarding_started = true;
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let settings = b.call("settings:get", vec![]).await.unwrap_or(Value::Null);
            let profiles = b.call("profiles:getAll", vec![]).await;
            let tour_state = b.call("tour:getState", vec![]).await.ok();
            let no_profiles = matches!(&profiles, Ok(Value::Array(a)) if a.is_empty());
            // First run only: no profiles AND setup never completed, so an upgrading user whose
            // settings predate the flag is never interrupted.
            let wizard = settings.get("setupCompleted") != Some(&Value::Bool(true)) && no_profiles;

            // Which tour is due, resolved before the notes are shown (they offer it).
            let mut pending = None;
            let mut bank: Vec<&'static str> = Vec::new();
            if let Some(state) = &tour_state {
                if state.get("forced") == Some(&Value::Bool(true)) {
                    pending = Some(latest_tour());
                } else {
                    let seen = str_list(state, "seenTourIds");
                    let unseen: Vec<usize> = (0..TOURS.len()).filter(|i| !seen.iter().any(|s| s == TOURS[*i].id)).collect();
                    if state.get("freshInstall") == Some(&Value::Bool(true)) {
                        // A first-ever launch gets the setup wizard, not a tour of features the
                        // user has no history with: bank every tour now.
                        bank = unseen.iter().map(|i| TOURS[*i].id).collect();
                    } else if let Some(&due) = unseen.last() {
                        // Several releases behind: only the newest tour; bank the rest.
                        bank = unseen.iter().filter(|i| **i != due).map(|i| TOURS[*i].id).collect();
                        pending = Some(due);
                    }
                }
            }
            for id in bank {
                let _ = b.call("tour:markSeen", vec![json!(id)]).await;
            }
            let _ = this.update(cx, |m, cx| {
                m.pending_tour = pending;
                if wizard {
                    m.wizard_mode = crate::wizard::WizardMode::FirstRun;
                    m.wizard_open = true;
                    m.wizard_seq += 1;
                }
                cx.notify();
            });

            let notes = b.call("changelog:getPending", vec![]).await;
            let notes = notes.ok().filter(|v| v.is_object());
            let _ = this.update(cx, |m, cx| {
                m.changelog = notes.map(|v| {
                    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    Changelog { version: s("version"), notes: s("notes"), url: s("url") }
                });
                m.changelog_checked = true;
                m.maybe_autostart_tour(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// With no release notes there is nothing to offer the tour from, so it starts on its own.
    pub fn maybe_autostart_tour(&mut self, cx: &mut gpui::Context<Self>) {
        if self.changelog_checked && self.changelog.is_none() && !self.wizard_open && self.tour.is_none() && self.pending_tour.is_some() {
            self.start_pending_tour(cx);
        }
    }

    fn mark_changelog_seen(&mut self) {
        if let Some(c) = self.changelog.take() {
            let _ = self.bridge.call("changelog:markSeen", vec![json!(c.version)]);
        }
    }

    /// "Got it" when no tour is due.
    pub fn dismiss_changelog(&mut self, cx: &mut gpui::Context<Self>) {
        self.mark_changelog_seen();
        cx.notify();
    }

    /// "Start tour": close the notes and go straight into it.
    pub fn start_tour_from_changelog(&mut self, cx: &mut gpui::Context<Self>) {
        self.mark_changelog_seen();
        self.start_pending_tour(cx);
        cx.notify();
    }

    /// "Skip tour" (and the X) on the notes: declining the offer banks the tour.
    pub fn skip_tour_from_changelog(&mut self, cx: &mut gpui::Context<Self>) {
        self.mark_changelog_seen();
        if let Some(t) = self.pending_tour.take() {
            let _ = self.bridge.call("tour:markSeen", vec![json!(TOURS[t].id)]);
        }
        cx.notify();
    }

    pub fn start_pending_tour(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(t) = self.pending_tour.take() else { return };
        let has_profiles = !self.profiles.is_empty();
        let steps: Vec<usize> = TOURS[t]
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| match s.needs {
                Needs::Always => true,
                Needs::Profiles => has_profiles,
            })
            .map(|(i, _)| i)
            .collect();
        self.form = None;
        self.tour = Some(ActiveTour { tour: t, steps, index: 0, start_page: self.page });
        self.enter_tour_step(cx);
    }

    fn enter_tour_step(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(t) = &self.tour else { return };
        let step = &TOURS[t.tour].steps[t.steps[t.index]];
        if let Some(page) = step.page {
            self.page = page;
        }
        if let Some(tab) = step.settings_tab {
            self.settings_tab_request = Some(tab);
        }
        cx.notify();
    }

    /// Next, or Done on the last step.
    pub fn tour_next(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(t) = &mut self.tour else { return };
        if t.index + 1 >= t.steps.len() {
            self.finish_tour(cx);
            return;
        }
        t.index += 1;
        self.enter_tour_step(cx);
    }

    pub fn tour_back(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(t) = &mut self.tour else { return };
        if t.index == 0 {
            return;
        }
        t.index -= 1;
        self.enter_tour_step(cx);
    }

    /// Done, Skip tour or Escape: either way the tour is marked seen.
    pub fn finish_tour(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(t) = self.tour.take() {
            let _ = self.bridge.call("tour:markSeen", vec![json!(TOURS[t.tour].id)]);
            self.page = t.start_page;
            cx.notify();
        }
    }

    /// The wizard closed (finished or skipped): record setup as done, then close it.
    pub fn finish_wizard(&mut self, cx: &mut gpui::Context<Self>) {
        let b = self.bridge.clone();
        let lock = self.save_lock();
        cx.spawn(async move |this, cx| {
            {
                let _guard = lock.lock().await;
                if let Ok(Value::Object(mut s)) = b.call("settings:get", vec![]).await {
                    if s.get("setupCompleted") != Some(&Value::Bool(true)) {
                        s.insert("setupCompleted".into(), Value::Bool(true));
                        let _ = b.call("settings:save", vec![Value::Object(s)]).await;
                    }
                }
            }
            let _ = this.update(cx, |m, cx| {
                m.wizard_open = false;
                m.reload(cx);
                m.maybe_autostart_tour(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn dismiss_paused_alert(&mut self, cx: &mut gpui::Context<Self>) {
        self.paused_alert = false;
        cx.notify();
    }
}

// -------------------------------------------------------------------------------------------------
// Rendering (drawn by the shell over the window)
// -------------------------------------------------------------------------------------------------

/// Plain text of the Markdown notes, for assistive technology (the rich text view is not in the
/// accessibility tree).
pub fn plain_text(md: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in md.lines() {
        let mut l = line.trim();
        if l.starts_with("```") || l.chars().all(|c| c == '-' || c == '*' || c == '_') && l.len() >= 3 {
            continue;
        }
        l = l.trim_start_matches('#').trim_start_matches('>').trim();
        for bullet in ["- ", "* ", "+ "] {
            if let Some(rest) = l.strip_prefix(bullet) {
                l = rest;
            }
        }
        if l.is_empty() {
            continue;
        }
        // [text](url) -> text; drop emphasis and code marks.
        let mut s = String::new();
        let mut rest = l;
        while let Some(open) = rest.find('[') {
            match (rest[open..].find("]("), rest[open..].find(')')) {
                (Some(mid), Some(close)) if mid < close => {
                    s.push_str(&rest[..open]);
                    s.push_str(&rest[open + 1..open + mid]);
                    rest = &rest[open + close + 1..];
                }
                _ => break,
            }
        }
        s.push_str(rest);
        let s = s.replace("**", "").replace("__", "").replace('`', "");
        out.push(s.trim_start_matches('!').to_string());
    }
    out.join("\n")
}

fn scrim(id: &'static str, p: &Palette) -> gpui::Stateful<gpui::Div> {
    div().id(id).absolute().inset_0().bg(p.scrim).flex().items_center().justify_center().p(px(16.))
}

fn open_external(b: &crate::bridge::Bridge, url: &str) {
    if !url.is_empty() {
        let _ = b.call("app:openExternal", vec![json!(url)]);
    }
}

/// 1.x `ChangelogModal`.
pub fn whats_new(model: &Entity<AppModel>, bridge: crate::bridge::Bridge, c: &Changelog, tour_offered: bool, p: &Palette, window: &Window) -> impl IntoElement {
    let m1 = model.clone();
    let m2 = model.clone();
    let m3 = model.clone();
    let close = move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
        m1.update(cx, |m, cx| if tour_offered { m.skip_tour_from_changelog(cx) } else { m.dismiss_changelog(cx) })
    };
    let close2 = close.clone();
    let link_bridge = bridge.clone();
    let url = c.url.clone();
    let notes_text = plain_text(&c.notes);
    let max_h = (window.viewport_size().height * 0.8).max(px(240.));

    let header = div()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(12.))
        .px(px(24.))
        .py(px(16.))
        .border_b_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .id("whats-new-title")
                        .role(Role::Heading)
                        .aria_label("What’s new")
                        .text_size(px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("What’s new"),
                )
                .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(format!("Version {}", c.version))),
        )
        .child(icon_button("whats-new-close", "x", "Close", p).on_press(close));

    let body =
        div().id("whats-new-body").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(24.)).py(px(20.)).text_size(px(size::TEXT)).text_color(p.text).child(
            div().id("whats-new-notes").role(Role::Label).aria_label(SharedString::from(notes_text.clone())).aria_value(SharedString::from(notes_text)).child(
                TextView::markdown("whats-new-markdown", c.notes.clone())
                    // 1.x heading sizes: 20 / 16 / 14 px.
                    .style(TextViewStyle::default().heading_font_size(|level, _| {
                        px(match level {
                            1 => 20.,
                            2 => 16.,
                            _ => 14.,
                        })
                    }))
                    .selectable(true)
                    // Remote images are not shown, as in 1.x.
                    .image_source(|_| gpui::ImageSource::from(std::path::PathBuf::new()))
                    .on_link_click(move |url, _, _, _| open_external(&link_bridge, url)),
            ),
        );

    let buttons = if tour_offered {
        div().flex().gap(px(8.)).child(secondary_button("whats-new-skip-tour", "Skip tour", None, p).on_press(close2)).child(
            primary_button("whats-new-start-tour", "Start tour", None, p).on_press(move |_, _, cx| m2.update(cx, |m, cx| m.start_tour_from_changelog(cx))),
        )
    } else {
        div().flex().child(primary_button("whats-new-got-it", "Got it", None, p).on_press(move |_, _, cx| m3.update(cx, |m, cx| m.dismiss_changelog(cx))))
    };
    let footer = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(24.))
        .py(px(16.))
        .border_t_1()
        .border_color(p.border)
        .child(
            div()
                .id("whats-new-github")
                .role(Role::Link)
                .aria_label("View on GitHub")
                .focusable()
                .tab_stop(true)
                .focus_ring(p)
                .px(px(6.))
                .py(px(2.))
                .ml(px(-6.))
                .rounded(px(6.))
                .text_size(px(size::TEXT_XS))
                .text_color(p.text_muted)
                .cursor_pointer()
                .hover(|d| d.text_color(p.text))
                .on_press(move |_, _, _| open_external(&bridge, &url))
                .child("View on GitHub"),
        )
        .child(buttons);

    let dialog = div()
        .id("whats-new-dialog")
        .role(Role::Dialog)
        .aria_label("What’s new")
        .on_click(|_, _, cx| cx.stop_propagation())
        .w(px(640.))
        .max_h(max_h)
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(size::RADIUS_LG + 2.))
        .border_1()
        .border_color(p.border)
        .bg(p.panel)
        .shadow_lg()
        .child(header)
        .child(body)
        .child(footer);
    crate::motion::fade_in(scrim("whats-new-scrim", p).child(crate::motion::dialog_in(dialog, "whats-new-in")), "whats-new-scrim-in", crate::motion::POPOVER)
}

/// 1.x `FeatureTour`'s step card. The overlay swallows clicks: the tour drives itself.
pub fn tour_card(model: &Entity<AppModel>, t: &ActiveTour, focus: &FocusHandle, p: &Palette) -> impl IntoElement {
    let step = &TOURS[t.tour].steps[t.steps[t.index]];
    let number = t.index + 1;
    let total = t.steps.len();
    let is_last = number == total;
    let name = format!("{} — step {number} of {total}", step.title);
    let (m1, m2, m3, m4) = (model.clone(), model.clone(), model.clone(), model.clone());

    let card = div()
        .id("tour-card")
        .role(Role::Dialog)
        .aria_label(SharedString::from(name))
        .track_focus(focus)
        .on_key_down(move |ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
            "escape" => m4.update(cx, |m, cx| m.finish_tour(cx)),
            "right" => m4.update(cx, |m, cx| m.tour_next(cx)),
            "left" => m4.update(cx, |m, cx| m.tour_back(cx)),
            _ => {}
        })
        .w(px(380.))
        .rounded(px(size::RADIUS_LG + 2.))
        .border_1()
        .border_color(p.border)
        .bg(p.panel)
        .shadow_lg()
        .child(
            div()
                .px(px(20.))
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .id("tour-title")
                                .role(Role::Heading)
                                .aria_label(step.title)
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(step.title),
                        )
                        .child(div().flex_shrink_0().mt(px(2.)).text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(format!("{number} / {total}"))),
                )
                .child(crate::widgets::read_text("tour-body", step.body).text_size(px(size::TEXT_SM)).text_color(p.text_muted).line_height(px(20.))),
        )
        .child(
            div()
                .mt(px(16.))
                .px(px(20.))
                .py(px(12.))
                .border_t_1()
                .border_color(p.border)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .id("tour-skip")
                        .role(Role::Button)
                        .aria_label("Skip tour")
                        .focusable()
                        .tab_stop(true)
                        .focus_ring(p)
                        .px(px(6.))
                        .py(px(2.))
                        .ml(px(-6.))
                        .rounded(px(6.))
                        .text_size(px(size::TEXT_XS))
                        .text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.text))
                        .on_press(move |_, _, cx| m1.update(cx, |m, cx| m.finish_tour(cx)))
                        .child("Skip tour"),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .when(t.index > 0, |d| {
                            d.child(
                                secondary_button("tour-back", "Back", None, p)
                                    .h(px(30.))
                                    .px(px(12.))
                                    .text_size(px(size::TEXT_SM))
                                    .on_press(move |_, _, cx| m2.update(cx, |m, cx| m.tour_back(cx))),
                            )
                        })
                        .child(
                            primary_button("tour-next", if is_last { "Done" } else { "Next" }, None, p)
                                .h(px(30.))
                                .px(px(12.))
                                .text_size(px(size::TEXT_SM))
                                .on_press(move |_, _, cx| m3.update(cx, |m, cx| m.tour_next(cx))),
                        ),
                ),
        );

    div()
        .id("tour-overlay")
        .absolute()
        .inset_0()
        .bg(p.scrim)
        .on_click(|_, _, cx| cx.stop_propagation())
        .flex()
        .flex_col()
        .items_center()
        .justify_end()
        .pb(px(32.))
        .child(crate::motion::dialog_in(card, "tour-card-in"))
}

/// 1.x Profiles "Auto-refresh paused" alert dialog.
pub fn paused_alert(model: &Entity<AppModel>, p: &Palette) -> impl IntoElement {
    let (m1, m2) = (model.clone(), model.clone());
    let scrim = scrim("paused-scrim", p).on_press(move |_, _, cx| m1.update(cx, |m, cx| m.dismiss_paused_alert(cx)));
    let dialog = div()
            .id("paused-dialog")
            .role(Role::AlertDialog)
            .aria_label("Auto-refresh paused")
            .on_click(|_, _, cx| cx.stop_propagation())
            .w(px(440.))
            .p(px(24.))
            .rounded(px(size::RADIUS_LG + 2.))
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child("Auto-refresh paused"))
            .child(crate::widgets::read_text(
                "paused-body",
                "You received multiple consecutive authentication failures, so automatic credential refresh was paused. Check your credentials (including a recent password change) before resuming auto-refresh in Settings.",
            ).text_size(px(size::TEXT_SM)).text_color(p.text_muted))
            .child(div().flex().justify_end().child(primary_button("paused-ok", "OK", None, p).on_press(move |_, _, cx| m2.update(cx, |m, cx| m.dismiss_paused_alert(cx)))));
    crate::motion::fade_in(scrim.child(crate::motion::dialog_in(dialog, "paused-in")), "paused-scrim-in", crate::motion::POPOVER)
}
