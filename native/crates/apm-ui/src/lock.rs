//! The lock screen (1.x `AuthScreen` in App.tsx): unlock with the master password, create one when
//! saved credentials are still unencrypted, or forget everything and start again.

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::{json, Value};

use crate::a11y::PressExt as _;
use crate::model::{AppModel, LockState};
use crate::theme::size;
use crate::widgets::{primary_button, secondary_button};

pub struct LockView {
    model: Entity<AppModel>,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<String>,
    busy: bool,
    forget_confirm: bool,
    _subs: Vec<Subscription>,
}

impl LockView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Enter password"));
        let confirm = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Repeat password"));
        password.update(cx, |s, cx| s.focus(window, cx));
        let subs = vec![cx.subscribe_in(&password, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.submit(cx)
            }
        })];
        LockView { model, password, confirm, error: None, busy: false, forget_confirm: false, _subs: subs }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let creating = self.model.read(cx).lock == LockState::NeedsCreate;
        let password = self.password.read(cx).value().to_string();
        let confirm = self.confirm.read(cx).value().to_string();
        if password.is_empty() {
            self.error = Some(if creating { "Please enter a password." } else { "Please enter your master password." }.into());
            cx.notify();
            return;
        }
        if creating {
            if password != confirm {
                self.error = Some("Passwords do not match".into());
                cx.notify();
                return;
            }
            if let Some(e) = apm_core::master_password::validate(&password) {
                self.error = Some(e);
                cx.notify();
                return;
            }
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let bridge = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = if creating {
                bridge.call("credentials:createMasterPassword", vec![json!(password), json!(confirm)]).await
            } else {
                bridge.call("credentials:unlock", vec![json!(password)]).await
            };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(v) if v.get("success") == Some(&Value::Bool(true)) => this.model.update(cx, |m, cx| m.unlocked(cx)),
                    Ok(v) => this.error = Some(v.get("error").and_then(Value::as_str).unwrap_or("Could not unlock").to_string()),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn forget_all(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        cx.notify();
        let bridge = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let _ = bridge.call("credentials:forgetAllAndResetMasterPassword", vec![]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.forget_confirm = false;
                this.model.update(cx, |m, cx| m.unlocked(cx));
            });
        })
        .detach();
    }
}

impl Render for LockView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let p = m.palette;
        // Over the background video (1.x: a frosted card over the playing video). GPUI cannot blur
        // behind the card, so it is a tint instead, opaque enough for its text over this video.
        let glass = m.glass_lock;
        let card_bg = if glass { gpui::Rgba { a: p.glass_alpha(m.glass_luminance, 0.72), ..p.panel } } else { p.panel };
        let card_border = if !glass {
            p.border
        } else if p.is_dark() {
            gpui::Rgba { r: 1., g: 1., b: 1., a: 0.2 }
        } else {
            gpui::Rgba { r: 0., g: 0., b: 0., a: 0.12 }
        };
        let creating = m.lock == LockState::NeedsCreate;
        let title: SharedString = if self.forget_confirm {
            "Remove all credentials?".into()
        } else if creating {
            "Master password".into()
        } else {
            "Unlock saved credentials".into()
        };
        let error = self.error.clone().map(|e| {
            div().id("lock-error").role(Role::Alert).aria_label(SharedString::from(e.clone())).text_size(px(size::TEXT_SM)).text_color(p.danger).child(e)
        });

        let card = div()
            .id("lock-dialog")
            .role(if self.forget_confirm { Role::AlertDialog } else { Role::Dialog })
            .aria_label(title.clone())
            .w(px(400.))
            .p(px(28.))
            .rounded(px(size::RADIUS_LG + 4.))
            .border_1()
            .border_color(card_border)
            .bg(card_bg)
            .when(glass, |d| d.shadow_lg())
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title));

        let card = if self.forget_confirm {
            card.child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child("All saved IdP credentials and your master password will be removed."))
                .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(
                    "You will be prompted for IdP login on each refresh until you save default credentials again. Saving them again will require a new master password.",
                ))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(secondary_button("lock-cancel", "Cancel", None, &p).on_press(cx.listener(|this, _, _, cx| {
                            this.forget_confirm = false;
                            cx.notify();
                        })))
                        .child(
                            primary_button("lock-remove-all", if self.busy { "Removing…" } else { "Remove all" }, None, &p)
                                .aria_label("Remove all")
                                .bg(p.danger_solid)
                                .on_press(cx.listener(|this, _, _, cx| this.forget_all(cx))),
                        ),
                )
        } else {
            let mut c = card
                .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(if creating {
                    "Saved credentials on this computer are not encrypted yet. Set a master password to protect them."
                } else {
                    "Enter your master password to access saved credentials."
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("Master password"))
                        .child(Input::new(&self.password).mask_toggle().aria_label("Master password")),
                );
            if creating {
                c = c.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("Confirm"))
                        .child(Input::new(&self.confirm).mask_toggle().aria_label("Confirm")),
                );
            }
            c = c.children(error).child(
                primary_button("lock-submit", if creating { "Set password" } else { "Unlock" }, None, &p)
                    .justify_center()
                    .on_press(cx.listener(|this, _, _, cx| this.submit(cx))),
            );
            if !creating {
                c = c.child(
                    div()
                        .id("lock-forgot")
                        .role(Role::Button)
                        .aria_label("Forgot master password?")
                        .focusable()
                        .self_center()
                        .text_size(px(size::TEXT_SM))
                        .text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.text))
                        .on_press(cx.listener(|this, _, _, cx| {
                            this.forget_confirm = true;
                            this.error = None;
                            cx.notify();
                        }))
                        .child("Forgot master password?"),
                );
            }
            c
        };

        div().size_full().when(!glass, |d| d.bg(p.bg)).flex().items_center().justify_center().child(card)
    }
}
