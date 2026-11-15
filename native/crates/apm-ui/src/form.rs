//! The profile add / edit form (1.x Profiles.tsx `editing` panel): name, credentials section,
//! sign-in method (SAML / Identity Center), the role or account picker with its loaders,
//! description, auto refresh and default credentials. Save and Cancel at the top and bottom.
//!
//! TODO(phase 4): the profile icon picker; until then the form keeps the icon a profile has.

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use serde_json::{json, Map, Value};

use crate::a11y::PressExt as _;
use crate::a11y::A11yDisabledExt;
use crate::dialogs::{CredPurpose, DialogKind};
use crate::model::AppModel;
use crate::picker::{Picker, PickerEvent, PickerOption};
use crate::theme::{size, Palette};
use crate::widgets::{icon_button, primary_button, secondary_button};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// 1.x `normalizeStartUrl`: drop a pasted `#/...` fragment and trailing slashes.
pub fn normalize_start_url(raw: &str) -> String {
    let t = raw.trim();
    let t = match t.find('#') {
        Some(i) => t[..i].trim_end_matches('/'),
        None => t,
    };
    t.trim_end_matches('/').to_string()
}

/// 1.x `roleToDisplayText`.
fn role_display(role: &Value, names: &Map<String, Value>) -> String {
    if let Some(a) = role.get("accountName").and_then(Value::as_str).map(str::trim).filter(|a| !a.is_empty()) {
        return a.to_string();
    }
    let arn = s(role, "roleArn");
    let parsed = arn.strip_prefix("arn:aws:iam::").and_then(|rest| rest.split_once(":role/"));
    match parsed {
        Some((account, role_name)) if account.chars().all(|c| c.is_ascii_digit()) && !account.is_empty() => {
            match names.get(account).and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => format!("{n} ({account}) - {role_name}"),
                None => format!("{account} / {role_name}"),
            }
        }
        _ => {
            let d = s(role, "displayText");
            if d.is_empty() { arn.to_string() } else { d.to_string() }
        }
    }
}

/// 1.x `ssoRoleDisplayText`.
fn sso_display(account: &Value, role_name: &str, names: &Map<String, Value>) -> String {
    let id = s(account, "accountId");
    let friendly = names
        .get(id)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .or_else(|| account.get("accountName").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()));
    match friendly {
        Some(f) => format!("{f} ({id}) - {role_name}"),
        None => format!("{id} / {role_name}"),
    }
}

pub struct FormView {
    model: Entity<AppModel>,
    is_new: bool,
    /// The profile as loaded; fields the form does not show (icon, order, folder) ride along.
    base: Value,
    sso: bool,
    auto_refresh: bool,
    use_default: bool,
    names: Map<String, Value>,
    sso_defaults: (String, String),
    name: Entity<InputState>,
    section: Entity<InputState>,
    idp: Entity<InputState>,
    sso_start: Entity<InputState>,
    sso_region: Entity<InputState>,
    description: Entity<InputState>,
    hours: Entity<InputState>,
    role: Entity<Picker>,
    sso_role: Entity<Picker>,
    roles: Option<Vec<Value>>,
    sso_accounts: Option<Vec<Value>>,
    sso_identity: Option<String>,
    /// The chosen role (SAML) / account and permission set (Identity Center).
    role_arn: Option<String>,
    sso_pick: Option<(String, String)>,
    /// Roles or accounts changed: rebuild the picker options on the next render.
    roles_changed: bool,
    busy: bool,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl FormView {
    /// `profile`: the full profile (edit) or a new one with defaults filled in (add).
    pub fn new(
        model: Entity<AppModel>,
        profile: Value,
        is_new: bool,
        names: Map<String, Value>,
        sso_defaults: (String, String),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = |value: &str, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            let value = value.to_string();
            cx.new(|cx| {
                let mut st = InputState::new(window, cx).placeholder(placeholder);
                st.set_value(value, window, cx);
                st
            })
        };
        let sso = s(&profile, "authType") == "identityCenter" || (!s(&profile, "ssoStartUrl").is_empty() && s(&profile, "idpEntryUrl").is_empty() && !is_new);
        let hours = (profile.get("refreshIntervalMinutes").and_then(Value::as_i64).unwrap_or(60) / 60).max(1);
        let name = text(s(&profile, "name"), "e.g. saml", window, cx);
        let section = text(s(&profile, "credentialProfileName"), "Same as profile name", window, cx);
        let idp = text(s(&profile, "idpEntryUrl"), "https://adfs.example.com/adfs/ls/...", window, cx);
        let sso_start = text(s(&profile, "ssoStartUrl"), "https://d-xxxxxxxxxx.awsapps.com/start", window, cx);
        let sso_region = text(s(&profile, "ssoRegion"), "us-west-2", window, cx);
        let description = text(s(&profile, "label"), "e.g. Production", window, cx);
        let hours = text(&hours.to_string(), "1", window, cx);
        let role = cx.new(|cx| {
            let mut p = Picker::new("role-picker", "Role / Account", cx);
            p.set_placeholder("Select a role…", cx);
            p
        });
        let sso_role = cx.new(|cx| Picker::new("sso-role-picker", "Account / Role", cx));

        let mut subs = vec![
            cx.observe(&model, |this: &mut Self, model, cx| {
                // Roles loaded through the "Sign in to load roles" dialog.
                if let Some((idp, roles)) = model.update(cx, |m, _| m.loaded_roles.take()) {
                    if idp == this.idp.read(cx).value().trim() {
                        this.roles = Some(roles);
                        this.roles_changed = true;
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&role, window, |this, _, ev: &PickerEvent, _, cx| {
                let PickerEvent::Changed(v) = ev;
                this.role_arn = v.clone();
                cx.notify();
            }),
            cx.subscribe_in(&sso_role, window, |this, _, ev: &PickerEvent, window, cx| {
                let PickerEvent::Changed(v) = ev;
                this.sso_pick = v.as_ref().and_then(|v| v.split_once('|')).map(|(a, r)| (a.to_string(), r.to_string()));
                // 1.x: a new Identity Center profile is named after the account and role.
                if let Some((account_id, role_name)) = this.sso_pick.clone() {
                    if this.name.read(cx).value().trim().is_empty() {
                        let account = this.sso_accounts.iter().flatten().find(|a| s(a, "accountId") == account_id).cloned();
                        if let Some(a) = account {
                            let n = format!("{} {role_name}", s(&a, "accountName"));
                            this.name.update(cx, |st, cx| st.set_value(n, window, cx));
                        }
                    }
                }
                cx.notify();
            }),
        ];
        // Observed, not only subscribed: an accessible SetValue edits the text without a Change event.
        for input in [&name, &section, &idp, &sso_start, &sso_region, &description, &hours] {
            subs.push(cx.observe(input, |_, _, cx| cx.notify()));
        }
        if is_new {
            // A new profile's section follows its name until the user edits the section.
            let mut last_name = s(&profile, "name").to_string();
            subs.push(cx.observe_in(&name, window, move |this, input, window, cx| {
                let new_name = input.read(cx).value().to_string();
                if new_name == last_name {
                    return;
                }
                let current = this.section.read(cx).value().to_string();
                if current.is_empty() || current == last_name {
                    let n = new_name.clone();
                    this.section.update(cx, |st, cx| st.set_value(n, window, cx));
                }
                last_name = new_name;
            }));
        }
        name.update(cx, |st, cx| st.focus(window, cx));

        let mut this = FormView {
            model,
            is_new,
            role_arn: profile.get("roleArn").and_then(Value::as_str).map(str::to_string),
            sso_pick: match (s(&profile, "ssoAccountId"), s(&profile, "ssoRoleName")) {
                (a, r) if !a.is_empty() && !r.is_empty() => Some((a.to_string(), r.to_string())),
                _ => None,
            },
            auto_refresh: profile.get("autoRefresh") == Some(&Value::Bool(true)),
            use_default: profile.get("useDefaultCredentials") == Some(&Value::Bool(true)),
            base: profile,
            sso,
            names,
            sso_defaults,
            name,
            section,
            idp,
            sso_start,
            sso_region,
            description,
            hours,
            role,
            sso_role,
            roles: None,
            sso_accounts: None,
            sso_identity: None,
            busy: false,
            error: None,
            // Build the pickers once so a saved role shows before any list loads.
            roles_changed: true,
            _subs: subs,
        };
        this.load_cached_roles(cx);
        this
    }

    fn bridge(&self, cx: &Context<Self>) -> crate::bridge::Bridge {
        self.model.read(cx).bridge.clone()
    }

    fn load_cached_roles(&mut self, cx: &mut Context<Self>) {
        let idp = self.idp.read(cx).value().trim().to_string();
        if idp.is_empty() {
            return;
        }
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let cached = b.call("roles:getCached", vec![json!(idp)]).await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Value::Array(roles)) = cached {
                    this.roles = Some(roles);
                    this.roles_changed = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 1.x `handleRefreshRoles`: default credentials, or ask ("Sign in to load roles").
    fn fetch_roles(&mut self, cx: &mut Context<Self>) {
        let idp = self.idp.read(cx).value().trim().to_string();
        if idp.is_empty() {
            self.error = Some("Set IdP entry URL first, then load roles.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        let id = (!self.is_new).then(|| s(&self.base, "id").to_string());
        let use_default = self.use_default;
        cx.spawn(async move |this, cx| {
            let r = b.call("auth:fetchRoles", vec![json!(idp), json!(use_default), json!(id)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) => {
                        if let Some(Value::Array(roles)) = v.get("roles") {
                            this.roles = Some(roles.clone());
                            this.roles_changed = true;
                        } else if v.get("credentialsRequired") == Some(&Value::Bool(true)) {
                            let prefill = s(&v, "prefillUsername").to_string();
                            this.model.update(cx, |m, cx| {
                                m.open_dialog(
                                    DialogKind::Credentials {
                                        purpose: CredPurpose::LoadRoles { idp_entry_url: idp },
                                        title: "Sign in to load roles".into(),
                                        description: "Enter your IdP username and password to fetch the list of roles for this IdP.".into(),
                                        prefill,
                                    },
                                    cx,
                                )
                            });
                        } else if v.get("success") == Some(&Value::Bool(false)) {
                            this.error = Some(s(&v, "error").to_string());
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 1.x `handleLoadSsoAccounts`: sign in if needed, then list accounts and permission sets.
    fn load_sso_accounts(&mut self, cx: &mut Context<Self>) {
        let start = normalize_start_url(&self.sso_start.read(cx).value());
        let region = self.sso_region.read(cx).value().trim().to_string();
        if start.is_empty() || region.is_empty() {
            self.error = Some("Set the SSO start URL and region first.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("sso:listAccounts", vec![json!(start), json!(region)]).await;
            let status = b.call("sso:getSessionStatus", vec![json!(start), json!(region)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) if v.get("error").is_some() => this.error = Some(s(&v, "error").to_string()),
                    Ok(v) => {
                        this.sso_accounts = Some(v.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default());
                        this.sso_identity = status.ok().and_then(|st| st.get("identity").and_then(Value::as_str).map(str::to_string));
                        this.roles_changed = true;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn detect_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start = normalize_start_url(&self.sso_start.read(cx).value());
        if start.is_empty() {
            self.error = Some("Enter the SSO start URL first.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        let region_input = self.sso_region.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let r = b.call("sso:detectRegion", vec![json!(start)]).await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    match r {
                        Ok(v) if v.get("region").is_some() => {
                            let region = s(&v, "region").to_string();
                            region_input.update(cx, |st, cx| st.set_value(region, window, cx));
                        }
                        Ok(v) => this.error = Some(s(&v, "error").to_string()),
                        Err(e) => this.error = Some(e),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn set_method(&mut self, sso: bool, window: &mut Window, cx: &mut Context<Self>) {
        if sso && !self.sso {
            // Fill the org from Settings on switching, without clobbering what is there.
            let (start, region) = self.sso_defaults.clone();
            if self.sso_start.read(cx).value().trim().is_empty() {
                self.sso_start.update(cx, |st, cx| st.set_value(start, window, cx));
            }
            if self.sso_region.read(cx).value().trim().is_empty() {
                self.sso_region.update(cx, |st, cx| st.set_value(region, window, cx));
            }
        }
        self.sso = sso;
        self.sso_accounts = None;
        self.sso_identity = None;
        self.roles_changed = true;
        cx.notify();
    }

    fn value(&self, input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().to_string()
    }

    fn valid(&self, cx: &Context<Self>) -> bool {
        let filled = |i: &Entity<InputState>| !self.value(i, cx).trim().is_empty();
        let common = filled(&self.name) && filled(&self.section);
        if self.sso {
            common && filled(&self.sso_start) && filled(&self.sso_region) && self.sso_pick.is_some()
        } else {
            common && filled(&self.idp) && self.role_arn.is_some()
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.valid(cx) {
            self.error = Some(if self.sso {
                "Please fill in all required fields: Profile name, Credentials section name, SSO start URL, SSO region, and Account / Role."
            } else {
                "Please fill in all required fields: Profile name, Credentials section name, IdP entry URL, and Role / Account."
            }
            .into());
            cx.notify();
            return;
        }
        let mut p = self.base.as_object().cloned().unwrap_or_default();
        let name = self.value(&self.name, cx);
        let section = self.value(&self.section, cx);
        p.insert("name".into(), json!(name));
        p.insert("credentialProfileName".into(), json!(if section.is_empty() { name.clone() } else { section }));
        p.insert("label".into(), json!(self.value(&self.description, cx)));
        p.insert("authType".into(), json!(if self.sso { "identityCenter" } else { "saml" }));
        p.insert("autoRefresh".into(), json!(self.auto_refresh));
        let hours = self.value(&self.hours, cx).trim().parse::<i64>().unwrap_or(1).clamp(1, 12);
        p.insert("refreshIntervalMinutes".into(), json!(hours * 60));
        p.insert("useDefaultCredentials".into(), json!(self.use_default));
        p.insert("idpEntryUrl".into(), json!(self.value(&self.idp, cx)));
        p.insert("ssoStartUrl".into(), json!(if self.sso { normalize_start_url(&self.value(&self.sso_start, cx)) } else { self.value(&self.sso_start, cx) }));
        p.insert("ssoRegion".into(), json!(self.value(&self.sso_region, cx).trim()));
        if self.sso {
            if let Some((account_id, role_name)) = &self.sso_pick {
                p.insert("ssoAccountId".into(), json!(account_id));
                p.insert("ssoRoleName".into(), json!(role_name));
                if let Some(a) = self.sso_accounts.iter().flatten().find(|a| s(a, "accountId") == account_id) {
                    p.insert("roleDisplayText".into(), json!(sso_display(a, role_name, &self.names)));
                }
            }
        } else if let Some(arn) = &self.role_arn {
            p.insert("roleArn".into(), json!(arn));
            if let Some(r) = self.roles.iter().flatten().find(|r| s(r, "roleArn") == arn) {
                p.insert("principalArn".into(), r.get("principalArn").cloned().unwrap_or(Value::Null));
                p.insert("roleDisplayText".into(), json!(role_display(r, &self.names)));
            }
        }
        self.model.update(cx, |m, cx| {
            m.close_form(cx);
            m.run("profiles:save", vec![Value::Object(p)], cx);
        });
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| m.close_form(cx));
    }

    /// Rebuild the picker options after roles or accounts load.
    fn sync_choices(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.roles_changed) {
            return;
        }
        let opt = |value: String, label: String| PickerOption { value, label: label.into() };
        let mut choices: Vec<PickerOption> =
            self.roles.iter().flatten().map(|r| opt(s(r, "roleArn").to_string(), role_display(r, &self.names))).collect();
        // Keep the saved role visible before the list is loaded.
        if let Some(arn) = &self.role_arn {
            if !choices.iter().any(|c| &c.value == arn) {
                let shown = s(&self.base, "roleDisplayText");
                choices.insert(0, opt(arn.clone(), if shown.is_empty() { arn.clone() } else { shown.to_string() }));
            }
        }
        let selected = self.role_arn.clone();
        self.role.update(cx, |p, cx| {
            p.set_options(choices, cx);
            p.set_selected(selected, cx);
        });

        let mut sso: Vec<PickerOption> = Vec::new();
        for a in self.sso_accounts.iter().flatten() {
            for role_name in a.get("roles").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                sso.push(opt(format!("{}|{role_name}", s(a, "accountId")), sso_display(a, role_name, &self.names)));
            }
        }
        let key = self.sso_pick.as_ref().map(|(a, r)| format!("{a}|{r}"));
        if let (Some(key), Some((a, r))) = (&key, &self.sso_pick) {
            if !sso.iter().any(|c| &c.value == key) {
                let shown = s(&self.base, "roleDisplayText");
                sso.insert(0, opt(key.clone(), if shown.is_empty() { format!("{a} / {r}") } else { shown.to_string() }));
            }
        }
        let placeholder = if self.sso_accounts.is_some() { "Select an account and role…" } else { "Sign in to load accounts…" };
        self.sso_role.update(cx, |p, cx| {
            p.set_options(sso, cx);
            p.set_selected(key, cx);
            p.set_placeholder(placeholder, cx);
        });
    }
}

fn label(text: &'static str, required: bool, p: &Palette) -> impl IntoElement {
    div()
        .flex()
        .gap(px(4.))
        .text_size(px(size::TEXT_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_muted)
        .child(text)
        .when(required, |d| d.child(div().text_color(p.danger).child("*")))
}

fn hint(text: &'static str, p: &Palette) -> impl IntoElement {
    div().text_size(px(size::TEXT_XS)).text_color(p.text_faint).child(text)
}

fn field(name: &'static str, required: bool, input: impl IntoElement, p: &Palette) -> gpui::Div {
    div().flex().flex_col().gap(px(6.)).child(label(name, required, p)).child(input)
}

impl Render for FormView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_choices(cx);
        let p = self.model.read(cx).palette;
        let valid = self.valid(cx);
        let busy = self.busy;
        let title = if self.is_new { "New profile" } else { "Edit profile" };

        let actions = |id: &'static str, cx: &mut Context<Self>| {
            div()
                .flex()
                .gap(px(8.))
                .child(
                    primary_button(SharedString::from(format!("{id}-save")), "Save", None, &p)
                        .when(!valid, |d| d.opacity(0.5).cursor_default())
                        .on_press(cx.listener(|this, _, _, cx| this.save(cx)))
                        .a11y_disabled(!valid),
                )
                .child(secondary_button(SharedString::from(format!("{id}-cancel")), "Cancel", None, &p).on_press(cx.listener(|this, _, _, cx| this.cancel(cx))))
        };

        let method = |id: &'static str, title: &'static str, blurb: &'static str, on: bool, sso: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .role(Role::Button)
                // Title and blurb, as the 1.x button read (its text content).
                .aria_label(format!("{title} {blurb}"))
                .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                .focusable()
                .flex_1()
                .px(px(12.))
                .py(px(10.))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(if on { p.accent } else { p.border })
                .bg(if on { p.selected } else { p.sunken })
                .cursor_pointer()
                .on_press(cx.listener(move |this, _, window, cx| this.set_method(sso, window, cx)))
                .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(if on { p.text } else { p.text_muted }).child(title))
                .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(blurb))
        };

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(div().flex_1().child(field("Profile name", true, Input::new(&self.name).aria_label("Profile name"), &p)))
                    .child(div().flex_1().child(field(
                        "Credentials section name",
                        true,
                        Input::new(&self.section).aria_label("Credentials section name"),
                        &p,
                    ))),
            )
            .child(hint("The section in your AWS credentials file. Use it with the AWS CLI --profile option.", &p))
            .child(
                div().flex().flex_col().gap(px(6.)).child(label("Sign-in method", false, &p)).child(
                    div()
                        .id("method")
                        .role(Role::Group)
                        .aria_label("Sign-in method")
                        .flex()
                        .gap(px(8.))
                        .child(method("method-saml", "SAML / ADFS", "Stored username + password, refreshed silently", !self.sso, false, cx))
                        .child(method("method-sso", "Identity Center", "Browser sign-in, then silent renewal", self.sso, true, cx)),
                ),
            );

        if !self.sso {
            body = body
                .child(field("IdP entry URL", true, Input::new(&self.idp).aria_label("IdP entry URL"), &p))
                .child(
                    field(
                        "Role / Account",
                        true,
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().child(self.role.clone()))
                            .child(icon_button("load-roles", "refresh-cw", "Load or refresh role list", &p).on_press(cx.listener(|this, _, _, cx| this.fetch_roles(cx)))),
                        &p,
                    )
                    .child(hint("Load the list by signing in; use default credentials or you will be prompted.", &p)),
                );
        } else {
            body = body
                .child(
                    field("SSO start URL", true, Input::new(&self.sso_start).aria_label("SSO start URL"), &p)
                        .child(hint("From the AWS access portal: any account, Access keys. A trailing #/ is fine.", &p)),
                )
                .child(field(
                    "SSO region",
                    true,
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(div().flex_1().child(Input::new(&self.sso_region).aria_label("SSO region")))
                        .child(secondary_button("detect-region", "Detect", None, &p).on_press(cx.listener(|this, _, window, cx| this.detect_region(window, cx)))),
                    &p,
                ))
                .child(
                    field(
                        "Account / Role",
                        true,
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().child(
                                self.sso_role.clone(),

                            ))
                            .child(
                                icon_button("load-accounts", "refresh-cw", "Sign in and load accounts", &p)
                                    .on_press(cx.listener(|this, _, _, cx| this.load_sso_accounts(cx))),
                            ),
                        &p,
                    )
                    .when_some(self.sso_identity.clone(), |d, who| {
                        d.child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(format!("Signed in as {who}")))
                    }),
                );
        }

        body = body
            .child(div().w(px(360.)).child(field("Description", false, Input::new(&self.description).aria_label("Description"), &p)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        Checkbox::new("auto-refresh")
                            .label("Auto refresh")
                            .checked(self.auto_refresh)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.auto_refresh = *checked;
                                cx.notify();
                            })),
                    )
                    .child(div().w(px(64.)).child(Input::new(&self.hours).aria_label("Refresh interval (hours)")))
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(if self.sso {
                        "hours (refresh interval)"
                    } else {
                        "hours (refresh interval and session length)"
                    })),
            );
        if self.sso {
            body = body.child(hint(
                "Identity Center profiles renew silently in the background. A browser sign-in is only needed when the SSO session itself expires.",
                &p,
            ));
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        Checkbox::new("use-default")
                            .label("Use default credentials")
                            .checked(self.use_default)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.use_default = *checked;
                                cx.notify();
                            })),
                    )
                    .child(hint(
                        "When on, refresh uses the username and password from Settings, Default credentials. When off, you are prompted on every refresh.",
                        &p,
                    )),
            );
        }
        // Errors sit under the heading, where the eye is after pressing a button at either end.
        let error = self.error.clone().map(|e| {
            div().id("form-error").role(Role::Alert).aria_label(SharedString::from(e.clone())).text_size(px(size::TEXT_SM)).text_color(p.danger).child(e)
        });
        if busy {
            body = body.child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child("Working…"));
        }

        div()
            .id("profile-form")
            .role(Role::Form)
            .aria_label(title)
            .size_full()
            .overflow_y_scroll()
            .bg(p.bg)
            .child(
                div()
                    .max_w(px(760.))
                    .px(px(24.))
                    .py(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().id("form-heading").role(Role::Heading).aria_label(title).text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(actions("top", cx)),
                    )
                    .children(error)
                    .child(body)
                    .child(actions("bottom", cx)),
            )
    }
}
