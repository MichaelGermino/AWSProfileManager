//! Modal dialogs over the main window: the IdP sign-in prompt (one profile, refresh-all, load roles),
//! the role picker, and confirmations. Ports of 1.x `CredentialsModal`, `RoleModal` and the
//! confirm dialogs, with the same titles (the UI flows find them by name).

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::{json, Value};

use crate::model::AppModel;
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{primary_button, secondary_button};

/// What a sign-in prompt is for.
#[derive(Debug, Clone)]
pub enum CredPurpose {
    /// `auth:submitCredentials` for one profile.
    Profile { id: String },
    /// `auth:submitCredentialsForRefreshAll`.
    RefreshAll { credential_ids: Vec<String>, default_ids: Vec<String> },
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteProfile { id: String },
    DeleteFolder { id: String },
}

#[derive(Debug, Clone)]
pub enum DialogKind {
    Credentials { purpose: CredPurpose, title: String, description: String, prefill: String },
    Roles { profile_id: String, profile_name: String, roles: Vec<Value> },
    Confirm { title: String, body: String, confirm: String, action: ConfirmAction },
}

pub struct DialogView {
    model: Entity<AppModel>,
    kind: DialogKind,
    username: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<String>,
    busy: bool,
    _subs: Vec<Subscription>,
}

impl DialogView {
    pub fn new(model: Entity<AppModel>, kind: DialogKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefill = match &kind {
            DialogKind::Credentials { prefill, .. } => prefill.clone(),
            _ => String::new(),
        };
        let username = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("you@company.com");
            s.set_value(prefill.clone(), window, cx);
            s
        });
        let password = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Password"));
        // Enter in either field submits, as the 1.x form did.
        let subs = vec![
            cx.subscribe_in(&username, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.submit(window, cx)
                }
            }),
            cx.subscribe_in(&password, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.submit(window, cx)
                }
            }),
        ];
        // Focus the first empty field.
        if matches!(kind, DialogKind::Credentials { .. }) {
            if prefill.is_empty() {
                username.update(cx, |s, cx| s.focus(window, cx));
            } else {
                password.update(cx, |s, cx| s.focus(window, cx));
            }
        }
        DialogView { model, kind, username, password, error: None, busy: false, _subs: subs }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind.clone();
        self.model.update(cx, |m, cx| m.dialog_closed(&kind, cx));
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let DialogKind::Credentials { purpose, .. } = self.kind.clone() else { return };
        let username = self.username.read(cx).value().to_string();
        let password = self.password.read(cx).value().to_string();
        if username.trim().is_empty() || password.is_empty() {
            self.error = Some("Enter your username and password.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let bridge = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = match &purpose {
                CredPurpose::Profile { id } => bridge.call("auth:submitCredentials", vec![json!(id), json!(username), json!(password)]).await,
                CredPurpose::RefreshAll { credential_ids, default_ids } => {
                    bridge
                        .call("auth:submitCredentialsForRefreshAll", vec![json!(credential_ids), json!(default_ids), json!(username), json!(password)])
                        .await
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match (&purpose, result) {
                    (_, Err(e)) => this.error = Some(e),
                    (CredPurpose::RefreshAll { .. }, Ok(_)) => this.close(cx),
                    (CredPurpose::Profile { id }, Ok(v)) => {
                        let id = id.clone();
                        this.model.update(cx, |m, cx| {
                            if let Some(err) = m.handle_refresh_result(&id, v, cx) {
                                // Still this dialog (wrong password): show why and stay open.
                                m.dialog_error = Some(err);
                            }
                        });
                        this.error = this.model.update(cx, |m, _| m.dialog_error.take());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_role(&mut self, index: usize, cx: &mut Context<Self>) {
        let DialogKind::Roles { profile_id, .. } = self.kind.clone() else { return };
        self.busy = true;
        cx.notify();
        let bridge = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.call("auth:selectRole", vec![json!(profile_id), json!(index)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.model.update(cx, |m, cx| {
                    m.close_dialog(cx);
                    m.refreshing.remove(&profile_id);
                    match result {
                        Ok(v) if v.get("success") == Some(&Value::Bool(false)) => {
                            let e = v.get("error").and_then(Value::as_str).unwrap_or("Refresh failed").to_string();
                            m.show_toast("Refresh failed", e, true, cx);
                        }
                        Err(e) => m.show_toast("Refresh failed", e, true, cx),
                        _ => {}
                    }
                    m.reload(cx);
                });
            });
        })
        .detach();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let DialogKind::Confirm { action, .. } = self.kind.clone() else { return };
        self.model.update(cx, |m, cx| {
            m.close_dialog(cx);
            match action {
                ConfirmAction::DeleteProfile { id } => {
                    if m.selected.as_deref() == Some(id.as_str()) {
                        m.selected = None;
                    }
                    m.run("profiles:delete", vec![json!(id)], cx)
                }
                ConfirmAction::DeleteFolder { id } => m.run("folders:delete", vec![json!(id)], cx),
            }
        });
    }
}

fn frame(id: &'static str, title: SharedString, alert: bool, p: &Palette) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(if alert { Role::AlertDialog } else { Role::Dialog })
        .aria_label(title)
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
}

fn field(label: &'static str, input: Input, p: &Palette) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(label))
        .child(input.aria_label(label))
}

impl Render for DialogView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.model.read(cx).palette;
        let busy = self.busy;
        let error = self.error.clone();
        let error_el = error.map(|e| {
            div().id("dialog-error").role(Role::Alert).aria_label(SharedString::from(e.clone())).text_size(px(size::TEXT_SM)).text_color(p.danger).child(e)
        });

        let body: gpui::AnyElement = match self.kind.clone() {
            DialogKind::Credentials { title, description, .. } => frame("credentials-dialog", title.clone().into(), false, &p)
                .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(description))
                .child(field("Username", Input::new(&self.username), &p))
                .child(field("Password", Input::new(&self.password).mask_toggle(), &p))
                .children(error_el)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(secondary_button("dialog-cancel", "Cancel", None, &p).on_click(cx.listener(|this, _, _, cx| this.close(cx))))
                        .child(
                            primary_button("dialog-submit", if busy { "Signing in…" } else { "Sign in" }, None, &p)
                                .aria_label("Sign in")
                                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                        ),
                )
                .into_any_element(),
            DialogKind::Roles { profile_name, roles, .. } => {
                let title = format!("Choose role for {profile_name}");
                let mut list = div().id("role-list").max_h(px(360.)).overflow_y_scroll().flex().flex_col().gap(px(4.));
                for (i, r) in roles.iter().enumerate() {
                    let arn = r.get("roleArn").and_then(Value::as_str).unwrap_or("").to_string();
                    let shown = r.get("displayText").and_then(Value::as_str).unwrap_or(&arn).to_string();
                    let account = r.get("accountName").and_then(Value::as_str).map(str::to_string);
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("role-{i}")))
                            .role(Role::Button)
                            .aria_label(SharedString::from(arn.clone()))
                            .focusable()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(size::RADIUS))
                            .border_1()
                            .border_color(p.border)
                            .cursor_pointer()
                            .hover(|d| d.bg(p.hover))
                            .on_click(cx.listener(move |this, _, _, cx| this.pick_role(i, cx)))
                            .when_some(account, |d, a| d.child(div().font_weight(FontWeight::MEDIUM).child(a)))
                            .child(div().font_family(FONT_MONO).text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(shown)),
                    );
                }
                frame("roles-dialog", title.clone().into(), false, &p)
                    .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child("This IdP offers several roles. The one you pick is remembered."))
                    .child(list)
                    .child(div().flex().justify_end().child(
                        secondary_button("dialog-cancel", "Cancel", None, &p).on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ))
                    .into_any_element()
            }
            DialogKind::Confirm { title, body, confirm, .. } => frame("confirm-dialog", title.clone().into(), true, &p)
                .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(body))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(secondary_button("dialog-cancel", "Cancel", None, &p).on_click(cx.listener(|this, _, _, cx| this.close(cx))))
                        .child(
                            primary_button("dialog-confirm", confirm.clone(), None, &p)
                                .bg(p.danger_solid)
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                        ),
                )
                .into_any_element(),
        };

        // Scrim; a click outside the dialog cancels, as in 1.x.
        div()
            .id("dialog-scrim")
            .absolute()
            .inset_0()
            .bg(p.scrim)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
            .child(div().id("dialog-body").on_click(|_, _, cx| cx.stop_propagation()).child(body))
    }
}
