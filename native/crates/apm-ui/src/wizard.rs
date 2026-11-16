//! The setup wizard (1.x `wizard/SetupWizard.tsx` and its steps), used two ways as in 1.x: on a
//! first run with every step, and as "Add accounts…" with only the import steps.
//!
//! Same steps, order, copy and skip rules as 1.x. The order is a real dependency chain: saving the
//! IdP sign-in needs a master password, and loading SAML roles uses the saved sign-in. The account
//! lists, the picker and the profile building are in `import.rs`.
//!
//! The Identity Center import calls `sso:listAccounts`, which signs in through
//! `Host::open_sign_in`; until that sign-in window exists the call reports an error, shown here.

use std::collections::HashSet;

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::{json, Map, Value};

use crate::a11y::{A11yDisabledExt, PressExt as _};
use crate::import::{self, ImportSource, PickerState};
use crate::model::AppModel;
use crate::picker::{Picker, PickerEvent, PickerOption};
use crate::theme::{size, Palette};
use crate::widgets::{icon, labelled, primary_button, read_text, secondary_button, FocusRing as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Step {
    Welcome,
    OrgConfig,
    ChooseType,
    MasterPassword,
    Credentials,
    SamlImport,
    SsoImport,
    Ai,
    Done,
}

/// First run or "Add accounts…" (1.x `WizardMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardMode {
    FirstRun,
    Import,
}

/// 1.x `FIRST_RUN_STEPS`. The type question comes first because everything after it depends on
/// the answer; the master password comes before the sign-in details because saving them needs
/// one, and before the Identity Center import because setting one drops stored SSO sessions.
const FIRST_RUN: &[Step] =
    &[Step::Welcome, Step::OrgConfig, Step::ChooseType, Step::MasterPassword, Step::Credentials, Step::SamlImport, Step::SsoImport, Step::Ai, Step::Done];

/// 1.x `IMPORT_STEPS`. The organization file leads here too (it filters itself out once answered).
const IMPORT: &[Step] = &[Step::OrgConfig, Step::ChooseType, Step::MasterPassword, Step::Credentials, Step::SamlImport, Step::SsoImport, Step::Done];

fn meta(s: Step) -> (&'static str, &'static str) {
    match s {
        Step::Welcome => ("Welcome", "sparkles"),
        Step::OrgConfig => ("Organization file", "building-2"),
        Step::ChooseType => ("Account type", "cloud"),
        Step::MasterPassword => ("Master password", "lock"),
        Step::Credentials => ("Sign-in details", "key-round"),
        Step::SamlImport => ("SAML accounts", "building-2"),
        Step::SsoImport => ("Identity Center", "id-card"),
        Step::Ai => ("AI assistant", "message-square"),
        Step::Done => ("Finish", "check"),
    }
}

const POINTS: [(&str, &str, &str); 3] = [
    ("key-round", "Short-lived credentials, refreshed for you", "Profiles write temporary keys to ~/.aws/credentials, so the AWS CLI and SDKs pick them up."),
    ("cloud", "SAML and IAM Identity Center side by side", "Import whole sets of accounts at once, from either sign-in method or both."),
    ("message-square", "Console and terminal for each profile", "Open the AWS console in one click, or a shell with the profile already set."),
];

pub struct WizardView {
    model: Entity<AppModel>,
    mode: WizardMode,
    current: Step,
    /// The step whose start-up (prefill from settings) has run; see `on_enter`.
    entered: Option<Step>,
    /// Profiles created during this run, for the summary.
    created: Vec<Value>,
    skipped: HashSet<Step>,
    want_saml: bool,
    want_sso: bool,
    master_set: bool,
    /// The master password existed before this step was reached (1.x `alreadySet`).
    master_already_set: bool,
    creds_saved: bool,
    org_answered: bool,
    org_config: Option<Map<String, Value>>,
    master: Entity<InputState>,
    master_confirm: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    api_url: Entity<InputState>,
    api_key: Entity<InputState>,
    api_key_saved: bool,
    models: Vec<String>,
    model_picker: Entity<Picker>,
    ai_model: Option<String>,
    // Import steps. Reset each time one is entered, as the 1.x step remounted.
    idp_url: Entity<InputState>,
    imp_user: Entity<InputState>,
    imp_pass: Entity<InputState>,
    sso_start: Entity<InputState>,
    sso_region: Entity<InputState>,
    filter: Entity<InputState>,
    picker: Option<PickerState>,
    /// SAML: nothing usable is stored, so the step asks for the sign-in itself.
    need_creds: bool,
    /// Default on: having just typed them, the user almost always wants them kept.
    remember: bool,
    can_remember: bool,
    saved_notice: bool,
    /// What a slow call is doing ("Signing in to your identity provider…").
    phase: Option<&'static str>,
    names: Map<String, Value>,
    error: Option<String>,
    busy: bool,
    _subs: Vec<Subscription>,
}

impl WizardView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |placeholder: &'static str, masked: bool, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).masked(masked).placeholder(placeholder))
        };
        let master = text("", true, window, cx);
        let master_confirm = text("", true, window, cx);
        let username = text("e.g. SA account", false, window, cx);
        let password = text("", true, window, cx);
        let api_url = text("https://your-instance.example.com/api", false, window, cx);
        let api_key = text("", true, window, cx);
        let idp_url = text("https://adfs.example.com/adfs/ls/...", false, window, cx);
        let imp_user = text("Username", false, window, cx);
        let imp_pass = text("Password", true, window, cx);
        let sso_start = text("https://d-xxxxxxxxxx.awsapps.com/start", false, window, cx);
        let sso_region = text("us-west-2", false, window, cx);
        let filter = text("Filter accounts…", false, window, cx);
        let model_picker = cx.new(|cx| {
            let mut p = Picker::new("wizard-ai-model", "Model", cx);
            p.set_placeholder("Select a model…", cx);
            p
        });
        let mut subs = Vec::new();
        // Observed, not only subscribed: an accessible SetValue edits the text without a Change event.
        for input in [&master, &master_confirm, &username, &password, &api_url, &api_key, &idp_url, &imp_user, &imp_pass, &sso_start, &sso_region, &filter] {
            subs.push(cx.observe(input, |this, _, cx| {
                this.error = None;
                cx.notify()
            }));
        }
        for input in [&master, &master_confirm] {
            subs.push(cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.set_master(window, cx)
                }
            }));
        }
        for input in [&username, &password] {
            subs.push(cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.save_credentials(window, cx)
                }
            }));
        }
        subs.push(cx.subscribe_in(&imp_pass, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.load_roles_with_credentials(window, cx)
            }
        }));
        subs.push(cx.subscribe(&model_picker, |this, _, ev: &PickerEvent, cx| {
            let PickerEvent::Changed(v) = ev;
            this.ai_model = v.clone().filter(|v| !v.is_empty());
            cx.notify();
        }));
        subs.push(cx.observe(&model, |_, _, cx| cx.notify()));

        let mode = model.read(cx).wizard_mode;
        let this = WizardView {
            model,
            mode,
            current: if mode == WizardMode::FirstRun { Step::Welcome } else { Step::OrgConfig },
            entered: None,
            created: Vec::new(),
            skipped: HashSet::new(),
            want_saml: false,
            want_sso: false,
            master_set: false,
            master_already_set: false,
            creds_saved: false,
            org_answered: false,
            org_config: None,
            master,
            master_confirm,
            username,
            password,
            api_url,
            api_key,
            api_key_saved: false,
            models: Vec::new(),
            model_picker,
            ai_model: None,
            idp_url,
            imp_user,
            imp_pass,
            sso_start,
            sso_region,
            filter,
            picker: None,
            need_creds: false,
            remember: true,
            can_remember: false,
            saved_notice: false,
            phase: None,
            names: Map::new(),
            error: None,
            busy: false,
            _subs: subs,
        };
        this.seed(window, cx);
        this
    }

    /// Seed from what is already configured, so steps that are done are not offered again.
    fn seed(&self, _window: &mut Window, cx: &mut Context<Self>) {
        let b = self.bridge(cx);
        cx.spawn_in(_window, async move |this, cx| {
            let mp = b.call("credentials:getMasterPasswordEnabled", vec![]).await.ok() == Some(Value::Bool(true));
            let creds = b.call("credentials:getDefaultDisplay", vec![]).await.unwrap_or(Value::Null);
            let st = b.call("settings:get", vec![]).await.unwrap_or(Value::Null);
            let _ = this.update_in(cx, |this, window, cx| {
                this.master_set = mp;
                this.master_already_set = mp;
                this.creds_saved = creds.get("hasPassword") == Some(&Value::Bool(true));
                let has = |k: &str| st.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
                this.org_answered = has("orgConfigImportedAt") || has("orgConfigDeclinedAt");
                this.api_key_saved = st.get("openWebUiApiKeySet") == Some(&Value::Bool(true));
                let url = st.get("openWebUiApiUrl").and_then(Value::as_str).unwrap_or("").to_string();
                this.api_url.update(cx, |s, cx| s.set_value(url, window, cx));
                this.ai_model = st.get("openWebUiModel").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                if this.api_key_saved {
                    this.api_key.update(cx, |s, cx| s.set_placeholder("Saved. Type a new key to replace it.", window, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn bridge(&self, cx: &Context<Self>) -> crate::bridge::Bridge {
        self.model.read(cx).bridge.clone()
    }

    fn visible(&self, s: Step) -> bool {
        match s {
            Step::OrgConfig => !self.org_answered,
            // Not SAML-only: the master password also encrypts Identity Center sessions and locks the app.
            Step::MasterPassword => (self.want_saml || self.want_sso) && !self.master_already_set,
            // SAML-only, and saving the IdP sign-in needs a master password.
            Step::Credentials => self.want_saml && self.master_set && !self.creds_saved,
            Step::SamlImport => self.want_saml,
            Step::SsoImport => self.want_sso,
            _ => true,
        }
    }

    fn order(&self) -> &'static [Step] {
        match self.mode {
            WizardMode::FirstRun => FIRST_RUN,
            WizardMode::Import => IMPORT,
        }
    }

    fn steps(&self) -> Vec<Step> {
        self.order().iter().copied().filter(|s| self.visible(*s) || *s == self.current).collect()
    }

    /// The first still-visible step after the current one, resolved by key after any change the
    /// step made on its way out (1.x tracks the key, never the position).
    fn next(&mut self, cx: &mut Context<Self>) {
        let order = self.order();
        let at = order.iter().position(|s| *s == self.current).unwrap_or(0);
        match order[at + 1..].iter().copied().find(|s| self.visible(*s)) {
            Some(s) => {
                self.current = s;
                self.error = None;
                cx.notify();
            }
            None => self.exit(cx),
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let order = self.order();
        let at = order.iter().position(|s| *s == self.current).unwrap_or(0);
        if let Some(s) = order[..at].iter().rev().copied().find(|s| self.visible(*s)) {
            self.current = s;
            self.error = None;
            cx.notify();
        }
    }

    fn skip(&mut self, cx: &mut Context<Self>) {
        self.skipped.insert(self.current);
        self.next(cx);
    }

    /// Leave the wizard entirely ("Skip setup", "Close", "Finish").
    fn exit(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| m.finish_wizard(cx));
    }

    // --- Organization file -----------------------------------------------------------------------

    fn choose_org_file(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("orgConfig:import", vec![]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) if v.get("canceled").is_some() => {}
                    Ok(v) if v.get("success") == Some(&Value::Bool(true)) => {
                        // Re-validate after the hop rather than trusting the shape that arrives.
                        match apm_core::org_config::parse(v.get("config").unwrap_or(&Value::Null)).filter(|c| !apm_core::org_config::is_empty(c)) {
                            Some(c) => this.org_config = Some(c),
                            None => this.error = Some("That file did not contain any settings this app can use.".into()),
                        }
                    }
                    Ok(v) => this.error = Some(v.get("error").and_then(Value::as_str).unwrap_or("Could not read that file.").to_string()),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fill-if-empty, so a re-run never overwrites choices already made.
    fn apply_org_config(&mut self, cx: &mut Context<Self>) {
        let Some(config) = self.org_config.clone() else { return };
        self.busy = true;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let current = b.call("settings:get", vec![]).await.ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
            let merged = apm_core::org_config::apply_to_settings(&current, &config, chrono::Utc::now());
            let r = b.call("settings:save", vec![Value::Object(merged)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(_) => {
                        this.model.update(cx, |m, cx| m.reload(cx));
                        this.next(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The "no" is recorded too, so later runs do not ask again.
    fn decline_org_config(&mut self, cx: &mut Context<Self>) {
        let mut patch = Map::new();
        patch.insert("orgConfigDeclinedAt".into(), json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
        self.model.update(cx, |m, cx| m.save_settings_patch(patch, cx));
        self.skip(cx);
    }

    // --- Master password and sign-in -------------------------------------------------------------

    fn set_master(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.current != Step::MasterPassword {
            return;
        }
        let password = self.master.read(cx).value().to_string();
        let confirm = self.master_confirm.read(cx).value().to_string();
        if password.is_empty() || confirm.is_empty() {
            return;
        }
        if let Some(e) = apm_core::master_password::validate(&password) {
            self.error = Some(e);
            cx.notify();
            return;
        }
        if password != confirm {
            self.error = Some("Passwords do not match".into());
            cx.notify();
            return;
        }
        self.busy = true;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("credentials:createMasterPassword", vec![json!(password), json!(confirm)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) if v.get("success") == Some(&Value::Bool(true)) => {
                        this.master_set = true;
                        this.next(cx);
                    }
                    Ok(v) => this.error = Some(v.get("error").and_then(Value::as_str).unwrap_or("Could not set the master password.").to_string()),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_credentials(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.current != Step::Credentials || !self.master_set {
            return;
        }
        let username = self.username.read(cx).value().trim().to_string();
        let password = self.password.read(cx).value().to_string();
        if username.is_empty() || password.is_empty() {
            return;
        }
        self.busy = true;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("credentials:setDefault", vec![json!(username), json!(password)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) if v.get("error").is_some() => this.error = Some("Could not save credentials. Set a master password first.".into()),
                    Ok(_) => {
                        this.creds_saved = true;
                        this.next(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // --- Import steps ----------------------------------------------------------------------------

    /// A step was just shown: start it fresh and prefill it from settings (1.x step `useEffect`).
    /// Prefills only empty fields, so a value typed before the settings arrive is kept.
    fn on_enter(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(step, Step::SamlImport | Step::SsoImport) {
            return;
        }
        self.picker = None;
        self.need_creds = false;
        self.saved_notice = false;
        self.phase = None;
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        self.imp_pass.update(cx, |s, cx| s.set_value("", window, cx));
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let settings = b.call("settings:get", vec![]).await.unwrap_or(Value::Null);
            let mp = b.call("credentials:getMasterPasswordEnabled", vec![]).await.ok() == Some(Value::Bool(true));
            let creds = b.call("credentials:getDefaultDisplay", vec![]).await.unwrap_or(Value::Null);
            let _ = this.update_in(cx, |this, window, cx| {
                let st = |k: &str| settings.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                this.names = settings.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
                let fill = |input: &Entity<InputState>, v: String, window: &mut Window, cx: &mut Context<Self>| {
                    if input.read(cx).value().is_empty() && !v.is_empty() {
                        input.update(cx, |s, cx| s.set_value(v, window, cx));
                    }
                };
                if step == Step::SamlImport {
                    fill(&this.idp_url, st("defaultIdpEntryUrl"), window, cx);
                    // Saving the sign-in encrypts it with the master password, so only with one.
                    this.can_remember = mp;
                    // Nothing stored: show the sign-in fields at once rather than after "Load roles".
                    if creds.get("hasPassword") != Some(&Value::Bool(true)) {
                        this.need_creds = true;
                        let user = if creds.get("locked") == Some(&Value::Bool(true)) {
                            String::new()
                        } else {
                            creds.get("username").and_then(Value::as_str).unwrap_or("").to_string()
                        };
                        fill(&this.imp_user, user, window, cx);
                    }
                } else {
                    fill(&this.sso_start, st("defaultSsoStartUrl"), window, cx);
                    fill(&this.sso_region, st("defaultSsoRegion"), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_accounts(&mut self, accounts: Vec<import::ImportableAccount>) {
        self.picker = Some(PickerState { accounts, selected: HashSet::new(), filter: self.filter.clone() });
    }

    fn receive_roles(&mut self, roles: &[Value]) {
        let accounts = import::from_saml_roles(roles, &self.names);
        let empty = accounts.is_empty();
        self.set_accounts(accounts);
        self.need_creds = false;
        if empty {
            self.error = Some("No roles were returned for this IdP URL.".into());
        }
    }

    /// "Load roles" with the saved sign-in; asks for one when nothing usable is stored.
    fn load_roles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.idp_url.read(cx).value().trim().to_string();
        if url.is_empty() {
            self.error = Some("Enter your IdP entry URL first.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        // Signing in walks the IdP redirect chain, then asks AWS for account names: several seconds.
        self.phase = Some("Signing in to your identity provider…");
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let r = b.call("auth:fetchRoles", vec![json!(url), json!(true)]).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.phase = None;
                match r {
                    Ok(v) => {
                        if let Some(Value::Array(roles)) = v.get("roles") {
                            this.receive_roles(roles);
                        } else if v.get("credentialsRequired") == Some(&Value::Bool(true)) {
                            this.need_creds = true;
                            if let Some(u) = v.get("prefillUsername").and_then(Value::as_str).filter(|u| !u.is_empty()) {
                                this.imp_user.update(cx, |s, cx| s.set_value(u.to_string(), window, cx));
                            }
                        } else if let Some(e) = v.get("error").and_then(Value::as_str) {
                            this.error = Some(e.to_string());
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Sign in & load roles" with a one-off sign-in; kept only after it worked.
    fn load_roles_with_credentials(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = self.idp_url.read(cx).value().trim().to_string();
        let user = self.imp_user.read(cx).value().trim().to_string();
        let pass = self.imp_pass.read(cx).value().to_string();
        if self.busy || self.current != Step::SamlImport || url.is_empty() || user.is_empty() || pass.is_empty() {
            return;
        }
        let remember = self.remember && self.can_remember;
        self.busy = true;
        self.error = None;
        self.phase = Some("Signing in and looking up your accounts…");
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn_in(_window, async move |this, cx| {
            let r = b.call("auth:fetchRolesWithCredentials", vec![json!(url), json!(user), json!(pass)]).await;
            let mut saved = false;
            if let Ok(Value::Object(o)) = &r {
                // Only after a successful sign-in: never keep credentials known to be wrong.
                if o.get("roles").is_some_and(Value::is_array) && remember {
                    let s = b.call("credentials:setDefault", vec![json!(user), json!(pass)]).await;
                    saved = matches!(&s, Ok(v) if v.get("error").is_none() && v.get("success") != Some(&Value::Bool(false)));
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.phase = None;
                match r {
                    Ok(v) => match v.get("roles") {
                        Some(Value::Array(roles)) => {
                            this.receive_roles(roles);
                            if saved {
                                this.saved_notice = true;
                                this.creds_saved = true;
                            }
                        }
                        _ => this.error = Some(v.get("error").and_then(Value::as_str).unwrap_or("Sign-in failed.").to_string()),
                    },
                    Err(e) => this.error = Some(e),
                }
                this.imp_pass.update(cx, |s, cx| s.set_value("", window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// Identity Center: sign in (a browser round-trip) and list the accounts and permission sets.
    fn load_sso(&mut self, cx: &mut Context<Self>) {
        let url = apm_core::sso_org::normalize_start_url(&self.sso_start.read(cx).value());
        let region = self.sso_region.read(cx).value().trim().to_string();
        if url.is_empty() || region.is_empty() {
            self.error = Some("Enter the SSO start URL and region.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("sso:listAccounts", vec![json!(url), json!(region)]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) => match (v.get("error").and_then(Value::as_str), v.get("accounts").and_then(Value::as_array)) {
                        (Some(e), _) => this.error = Some(e.to_string()),
                        (None, accounts) => {
                            let accounts = import::from_sso_accounts(accounts.map(Vec::as_slice).unwrap_or(&[]), &this.names);
                            if accounts.is_empty() {
                                // Signed in, nothing assigned: almost always "not provisioned yet".
                                this.error = Some("Signed in successfully, but no AWS accounts are assigned to you yet. Ask whoever manages Identity Center to grant you access, then try again. You can skip this and add them later.".into());
                            }
                            this.set_accounts(accounts);
                        }
                    },
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Create N profiles" on either import step.
    fn create_profiles(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = &self.picker else { return };
        if self.busy || picker.selected.is_empty() {
            return;
        }
        let source = if self.current == Step::SsoImport {
            ImportSource::IdentityCenter {
                start_url: apm_core::sso_org::normalize_start_url(&self.sso_start.read(cx).value()),
                region: self.sso_region.read(cx).value().trim().to_string(),
            }
        } else {
            ImportSource::Saml { idp_entry_url: self.idp_url.read(cx).value().trim().to_string() }
        };
        let (accounts, selected) = (picker.accounts.clone(), picker.selected.clone());
        self.busy = true;
        self.phase = Some("Creating profiles…");
        cx.notify();
        let b = self.bridge(cx);
        let lock = self.model.read(cx).save_lock();
        cx.spawn(async move |this, cx| {
            let r = import::persist_import(b, lock, accounts, selected, source).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.phase = None;
                match r {
                    Ok(created) => {
                        this.created.extend(created);
                        this.model.update(cx, |m, cx| m.reload(cx));
                        this.next(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // --- AI assistant ----------------------------------------------------------------------------

    /// Saving first is required: the models call reads the URL and key from settings.
    fn load_models(&mut self, cx: &mut Context<Self>) {
        let url = self.api_url.read(cx).value().trim().to_string();
        let key = self.api_key.read(cx).value().trim().to_string();
        self.busy = true;
        self.error = None;
        cx.notify();
        let b = self.bridge(cx);
        let lock = self.model.read(cx).save_lock();
        cx.spawn(async move |this, cx| {
            {
                let _guard = lock.lock().await;
                let mut st = b.call("settings:get", vec![]).await.ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
                st.insert("openWebUiApiUrl".into(), json!(url));
                // Empty field = keep the saved key.
                if !key.is_empty() {
                    st.insert("openWebUiApiKey".into(), json!(key));
                }
                let _ = b.call("settings:save", vec![Value::Object(st)]).await;
            }
            let r = b.call("ai:getModels", vec![]).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match r {
                    Ok(v) if v.get("error").is_some() => this.error = v.get("error").and_then(Value::as_str).map(str::to_string),
                    Ok(v) => {
                        this.models = v.get("models").and_then(Value::as_array).into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
                        if this.models.is_empty() {
                            this.error = Some("The instance returned no models.".into());
                        }
                        let opts = this.models.iter().map(|m| PickerOption { value: m.clone(), label: m.clone().into() }).collect();
                        let selected = this.ai_model.clone().filter(|m| this.models.contains(m));
                        this.ai_model = selected.clone();
                        this.model_picker.update(cx, |p, cx| {
                            p.set_options(opts, cx);
                            p.set_selected(selected, cx);
                        });
                    }
                    Err(e) => this.error = Some(e),
                }
                this.model.update(cx, |m, cx| m.reload(cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn save_model(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.ai_model.clone() else { return };
        let mut patch = Map::new();
        patch.insert("openWebUiModel".into(), json!(m));
        self.model.update(cx, |model, cx| model.save_settings_patch(patch, cx));
        self.next(cx);
    }
}

// -------------------------------------------------------------------------------------------------
// Render
// -------------------------------------------------------------------------------------------------

fn header(title: &'static str, blurb: Option<String>, p: &Palette) -> gpui::Div {
    div()
        .mb(px(24.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .id("wizard-step-heading")
                .role(Role::Heading)
                .aria_label(title)
                .text_size(px(24.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text)
                .child(title),
        )
        .when_some(blurb, |d, b| d.child(read_text("wizard-step-blurb", b).max_w(px(640.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted)))
}

/// A text button (1.x `btn-ghost` / the muted Back link).
fn ghost(id: &'static str, label: impl Into<SharedString>, p: &Palette) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_stop(true)
        .focus_ring(p)
        .h(px(size::BUTTON))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(size::RADIUS))
        .text_size(px(size::TEXT))
        .text_color(p.text_muted)
        .cursor_pointer()
        .hover(|d| d.bg(p.hover).text_color(p.text))
        .child(label)
}

impl WizardView {
    /// 1.x `StepFooter`: main button, skip, and Back on the right.
    fn footer(
        &self,
        next: Option<(SharedString, bool, fn(&mut Self, &mut Window, &mut Context<Self>))>,
        skip: Option<(String, fn(&mut Self, &mut Window, &mut Context<Self>))>,
        back: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let busy = self.busy;
        let mut row = div().mt(px(32.)).flex().items_center().gap(px(8.));
        let next_present = next.is_some();
        if let Some((label, enabled, f)) = next {
            let enabled = enabled && !busy;
            row = row.child(
                primary_button("wizard-next", if busy { SharedString::from("Working…") } else { label.clone() }, None, p)
                    .aria_label(label)
                    .when(!enabled, |d| d.opacity(0.5).cursor_default())
                    .on_press(cx.listener(move |this, _, window, cx| {
                        if enabled {
                            f(this, window, cx)
                        }
                    }))
                    .a11y_disabled(!enabled),
            );
        }
        let has_next = next_present;
        if let Some((label, f)) = skip {
            // Alone in the row, the text lines up with the content edge.
            row = row.child(ghost("wizard-skip", label, p).when(!has_next, |d| d.ml(px(-12.))).on_press(cx.listener(move |this, _, window, cx| {
                if !this.busy {
                    f(this, window, cx)
                }
            })));
        }
        if back {
            row = row.child(div().flex_1()).child(ghost("wizard-back", "Back", p).on_press(cx.listener(|this, _, _, cx| {
                if !this.busy {
                    this.back(cx)
                }
            })));
        }
        row
    }

    /// What follows the inputs on both import steps: the picker, progress, notices, the error and
    /// the footer with "Create N profiles".
    fn import_tail(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut out = Vec::new();
        let count = self.picker.as_ref().map(|s| s.selected.len()).unwrap_or(0);
        let has_accounts = self.picker.as_ref().is_some_and(|s| !s.accounts.is_empty());
        if let Some(state) = self.picker.as_ref().filter(|s| !s.accounts.is_empty()) {
            out.push(div().mt(px(24.)).max_w(px(640.)).child(import::render_picker(state, picker_of, p, cx)).into_any_element());
        }
        if let (true, Some(phase)) = (self.busy, self.phase) {
            out.push(
                div()
                    .id("wizard-phase")
                    .role(Role::Status)
                    .aria_label(phase)
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(size::TEXT_SM))
                    .text_color(p.text_muted)
                    .child(icon("refresh-cw", 14., p.text_muted))
                    .child(phase)
                    .into_any_element(),
            );
        }
        if self.saved_notice {
            out.push(
                read_text("wizard-saved-notice", "Sign-in details saved. Profiles will refresh in the background without asking again.")
                    .mt(px(16.))
                    .max_w(px(640.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.panel)
                    .text_size(px(size::TEXT_SM))
                    .text_color(p.text_muted)
                    .into_any_element(),
            );
        }
        if let Some(e) = self.error_line(p) {
            out.push(e.into_any_element());
        }
        let label: SharedString = format!("Create {count} profile{}", if count == 1 { "" } else { "s" }).into();
        out.push(
            self.footer(
                has_accounts.then_some((label, count > 0, |t: &mut Self, _: &mut Window, cx: &mut Context<Self>| t.create_profiles(cx))),
                Some(("Skip".into(), |t, _, cx| t.skip(cx))),
                true,
                p,
                cx,
            )
            .into_any_element(),
        );
        out
    }

    fn error_line(&self, p: &Palette) -> Option<gpui::Stateful<gpui::Div>> {
        self.error.clone().map(|e| {
            div()
                .id("wizard-error")
                .role(Role::Alert)
                .aria_label(SharedString::from(e.clone()))
                .mt(px(16.))
                .max_w(px(640.))
                .px(px(16.))
                .py(px(12.))
                .rounded(px(size::RADIUS_LG))
                .border_1()
                .border_color(p.danger)
                .text_size(px(size::TEXT_SM))
                .text_color(p.danger)
                .child(e)
        })
    }

    fn render_step(&mut self, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let body = div().flex().flex_col();
        match self.current {
            Step::Welcome => {
                let mut list = div().flex().flex_col().max_w(px(640.)).border_t_1().border_color(p.border);
                for (i, (ic, title, text)) in POINTS.iter().enumerate() {
                    list = list.child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .py(px(16.))
                            .border_b_1()
                            .border_color(p.border)
                            .child(div().mt(px(2.)).child(icon(ic, 20., p.text_muted)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        read_text(SharedString::from(format!("welcome-point-{i}")), *title)
                                            .text_size(px(size::TEXT_SM))
                                            .font_weight(FontWeight::MEDIUM),
                                    )
                                    .child(
                                        read_text(SharedString::from(format!("welcome-point-body-{i}")), *text)
                                            .text_size(px(size::TEXT_SM))
                                            .text_color(p.text_muted),
                                    ),
                            ),
                    );
                }
                body.child(header(
                    "Welcome to AWS Profile Manager",
                    Some("Set up your accounts in a few minutes. Every step can be skipped and changed later in Settings.".into()),
                    p,
                ))
                .child(list)
                .child(self.footer(
                    Some(("Get started".into(), true, |t, _, cx| t.next(cx))),
                    Some(("Skip setup".into(), |t, _, cx| t.exit(cx))),
                    false,
                    p,
                    cx,
                ))
            }
            Step::OrgConfig => {
                let chosen = self.org_config.clone();
                let choose = div()
                    .id("org-choose")
                    .role(Role::Button)
                    .aria_label(if chosen.is_some() { "Choose a different file…" } else { "Choose a configuration file…" })
                    .focusable()
                    .tab_stop(true)
                    .focus_ring(p)
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .p(px(24.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_dashed()
                    .border_color(p.border_strong)
                    .bg(p.panel)
                    .cursor_pointer()
                    .hover(|d| d.border_color(p.accent))
                    .on_press(cx.listener(|this, _, _, cx| {
                        if !this.busy {
                            this.choose_org_file(cx)
                        }
                    }))
                    .child(icon("sparkles", 24., p.text_muted))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child(if chosen.is_some() {
                                "Choose a different file…"
                            } else {
                                "Choose a configuration file…"
                            }))
                            .child(
                                div()
                                    .text_size(px(size::TEXT_SM))
                                    .text_color(p.text_muted)
                                    .child("A .json file exported from this app by someone in your organization."),
                            ),
                    );
                let summary =
                    chosen.as_ref().map(|c| {
                        let name = c.get("organizationName").and_then(Value::as_str).unwrap_or("Organization configuration").to_string();
                        let mut d = div()
                            .mt(px(16.))
                            .p(px(16.))
                            .rounded(px(size::RADIUS_LG))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.panel)
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(read_text("org-name", name).text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM))
                            .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child("This file provides:"));
                        for (i, line) in apm_core::org_config::describe(c).into_iter().enumerate() {
                            d =
                                d.child(
                                    div().flex().items_center().gap(px(8.)).child(icon("check", 14., p.text_muted)).child(
                                        read_text(SharedString::from(format!("org-line-{i}")), line).text_size(px(size::TEXT_SM)).text_color(p.text_muted),
                                    ),
                                );
                        }
                        d
                    });
                let has = chosen.is_some();
                body.child(header(
                    "Do you have an organization configuration?",
                    Some("If someone on your team shared a configuration file, load it here and the rest of setup will be filled in for you. It contains URLs and account names only — never passwords or keys.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(640.))
                        .child(choose)
                        .child(read_text(
                            "org-help",
                            "Don’t have one? Ask a teammate who already uses this app to open Settings → General → Organization configuration and click Export…, then send you the file. It holds only URLs and account names — no passwords, keys or credentials — so it is safe to share. You can also skip this and enter the details yourself.",
                        ).mt(px(12.)).text_size(px(size::TEXT_XS)).text_color(p.text_muted))
                        .children(summary),
                )
                .children(self.error_line(p))
                .child(self.footer(
                    has.then_some(("Use this configuration".into(), true, |t: &mut Self, _: &mut Window, cx: &mut Context<Self>| t.apply_org_config(cx))),
                    Some((if has { "Skip".into() } else { "I don't have one".into() }, |t, _, cx| t.decline_org_config(cx))),
                    false,
                    p,
                    cx,
                ))
            }
            Step::ChooseType => {
                let option = |id: &'static str, ic: &'static str, title: &'static str, text: &'static str, on: bool, saml: bool, cx: &mut Context<Self>| {
                    div()
                        .id(id)
                        .role(Role::Button)
                        .aria_label(title)
                        .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                        .focusable()
                        .tab_stop(true)
                        .focus_ring(p)
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(20.))
                        .rounded(px(size::RADIUS_LG))
                        .border_1()
                        .border_color(if on { p.accent } else { p.border })
                        .bg(if on { p.selected } else { p.panel })
                        .cursor_pointer()
                        .when(!on, |d| d.hover(|d| d.border_color(p.text_muted)))
                        .on_press(cx.listener(move |this, _, _, cx| {
                            if saml {
                                this.want_saml = !this.want_saml;
                            } else {
                                this.want_sso = !this.want_sso;
                            }
                            cx.notify();
                        }))
                        .child(icon(ic, 28., if on { p.accent } else { p.text_muted }))
                        .child(div().mt(px(12.)).text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child(title))
                        .child(div().mt(px(4.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(text))
                };
                let any = self.want_saml || self.want_sso;
                body.child(header(
                    "How do you sign in to AWS?",
                    Some("Pick either or both. Not sure? Your AWS access portal URL looks like d-xxxxxxxxxx.awsapps.com for Identity Center; anything else is probably SAML.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(640.))
                        .flex()
                        .gap(px(16.))
                        .child(option("type-saml", "building-2", "SAML / ADFS", "Sign in with your IdP username and password. Refreshes silently in the background.", self.want_saml, true, cx))
                        .child(option("type-sso", "id-card", "IAM Identity Center", "Sign in once in a browser. Renews silently until the SSO session expires.", self.want_sso, false, cx)),
                )
                .child(self.footer(Some(("Continue".into(), any, |t, _, cx| t.next(cx))), Some(("I'll add accounts later".into(), |t, _, cx| t.skip(cx))), false, p, cx))
            }
            Step::MasterPassword => {
                if self.master_already_set {
                    return body
                        .child(header("Master password", Some("You already have one set, so there is nothing to do here.".into()), p))
                        .child(self.footer(Some(("Continue".into(), true, |t, _, cx| t.next(cx))), None, true, p, cx));
                }
                let filled = !self.master.read(cx).value().is_empty() && !self.master_confirm.read(cx).value().is_empty();
                let blurb = format!(
                    "Encrypts what this app stores on your machine (saved sign-in details and Identity Center sessions) and locks the app after a restart until you enter it. It is never stored. If you forget it, sign in again to rebuild what was saved.{}",
                    if self.want_saml { " Saving your IdP sign-in for automatic refresh needs one." } else { "" }
                );
                let skip_label = if self.want_saml { "Skip, ask me to sign in each time" } else { "Skip" };
                body.child(header("Set a master password", Some(blurb), p))
                    .child(
                        div()
                            .max_w(px(448.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(labelled("Master password", Input::new(&self.master).mask_toggle(), p))
                            .child(labelled("Confirm", Input::new(&self.master_confirm).mask_toggle(), p)),
                    )
                    .children(self.error_line(p))
                    .child(self.footer(
                        Some(("Set password".into(), filled, |t, w, cx| t.set_master(w, cx))),
                        Some((skip_label.into(), |t, _, cx| t.skip(cx))),
                        true,
                        p,
                        cx,
                    ))
            }
            Step::Credentials => {
                if !self.master_set {
                    return body
                        .child(header(
                            "Sign-in details",
                            Some("Saving your IdP username and password needs a master password to encrypt them, and that step was skipped. You can still add SAML accounts — you will just be asked to sign in each time instead.".into()),
                            p,
                        ))
                        .child(self.footer(Some(("Continue".into(), true, |t, _, cx| t.next(cx))), None, true, p, cx));
                }
                let filled = !self.username.read(cx).value().trim().is_empty() && !self.password.read(cx).value().is_empty();
                body.child(header(
                    "Your IdP sign-in",
                    Some("Used to fetch your list of SAML roles and to refresh those profiles in the background. Stored encrypted on this machine and never sent anywhere except your identity provider.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(448.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(labelled("Username", Input::new(&self.username), p))
                        .child(labelled("Password", Input::new(&self.password).mask_toggle(), p)),
                )
                .children(self.error_line(p))
                .child(self.footer(Some(("Save".into(), filled, |t, w, cx| t.save_credentials(w, cx))), Some(("Skip".into(), |t, _, cx| t.skip(cx))), true, p, cx))
            }
            Step::SamlImport => {
                let url_ok = !self.idp_url.read(cx).value().trim().is_empty();
                let can_load = !self.busy && url_ok;
                let mut url_row = div().flex().gap(px(8.)).child(div().flex_1().child(Input::new(&self.idp_url).aria_label("IdP entry URL")));
                if !self.need_creds {
                    url_row = url_row.child(
                        secondary_button("wizard-load-roles", if self.busy { "Loading…" } else { "Load roles" }, None, p)
                            .h(px(size::CONTROL))
                            .text_size(px(size::TEXT_SM))
                            .aria_label("Load roles")
                            .when(!can_load, |d| d.opacity(0.5).cursor_default())
                            .on_press(cx.listener(move |this, _, window, cx| {
                                if can_load {
                                    this.load_roles(window, cx)
                                }
                            }))
                            .a11y_disabled(!can_load),
                    );
                }
                let creds_box = self.need_creds.then(|| {
                    let ready = !self.busy && url_ok && !self.imp_user.read(cx).value().trim().is_empty() && !self.imp_pass.read(cx).value().is_empty();
                    let remember_on = self.remember && self.can_remember;
                    let can_remember = self.can_remember;
                    let remember_text = if can_remember {
                        "Remember these so profiles refresh in the background without asking again."
                    } else {
                        "Set a master password in Settings to save these; otherwise you will be asked each time credentials are needed."
                    };
                    div()
                        .mt(px(16.))
                        .max_w(px(448.))
                        .p(px(16.))
                        .rounded(px(size::RADIUS_LG))
                        .border_1()
                        .border_color(p.border)
                        .bg(p.panel)
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            read_text("saml-signin-hint", "Sign in to your identity provider to load your roles.")
                                .text_size(px(size::TEXT_SM))
                                .text_color(p.text_muted),
                        )
                        .child(Input::new(&self.imp_user).aria_label("Username"))
                        .child(Input::new(&self.imp_pass).mask_toggle().aria_label("Password"))
                        .child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .id("saml-remember")
                                        .role(Role::CheckBox)
                                        .aria_label(remember_text)
                                        .aria_toggled(if remember_on { gpui::Toggled::True } else { gpui::Toggled::False })
                                        .focusable()
                                        .tab_stop(true)
                                        .focus_ring(p)
                                        .mt(px(1.))
                                        .size(px(16.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(if remember_on { p.primary } else { p.border_strong })
                                        .when(remember_on, |d| d.bg(p.primary).child(icon("check", 12., p.on_primary)))
                                        .when(!can_remember, |d| d.opacity(0.5))
                                        .when(can_remember, |d| d.cursor_pointer())
                                        .on_press(cx.listener(move |this, _, _, cx| {
                                            if can_remember {
                                                this.remember = !this.remember;
                                                cx.notify();
                                            }
                                        }))
                                        .a11y_disabled(!can_remember),
                                )
                                .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(remember_text)),
                        )
                        .child(
                            div().mt(px(4.)).flex().child(
                                primary_button("wizard-signin-roles", if self.busy { "Signing in…" } else { "Sign in & load roles" }, None, p)
                                    .aria_label("Sign in & load roles")
                                    .when(!ready, |d| d.opacity(0.5).cursor_default())
                                    .on_press(cx.listener(move |this, _, window, cx| {
                                        if ready {
                                            this.load_roles_with_credentials(window, cx)
                                        }
                                    }))
                                    .a11y_disabled(!ready),
                            ),
                        )
                });
                body.child(header(
                    "Import SAML accounts",
                    Some("Sign in to your identity provider and pick which account and role pairs to create profiles for.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(640.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("IdP entry URL"))
                        .child(url_row),
                )
                .children(creds_box)
                .children(self.import_tail(p, cx))
            }
            Step::SsoImport => {
                let ready = !self.busy && !self.sso_start.read(cx).value().trim().is_empty() && !self.sso_region.read(cx).value().trim().is_empty();
                body.child(header(
                    "Import Identity Center accounts",
                    Some("From the AWS access portal: open any account, choose Access keys, and copy the two values from the AWS IAM Identity Center credentials tab.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(640.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(labelled("SSO start URL", Input::new(&self.sso_start), p))
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap(px(8.))
                                .child(div().flex_1().child(labelled("SSO region", Input::new(&self.sso_region), p)))
                                .child(
                                    secondary_button("wizard-sso-load", if self.busy { "Waiting for browser…" } else { "Sign in & load" }, None, p)
                            .h(px(size::CONTROL))
                            .text_size(px(size::TEXT_SM))
                                        .aria_label("Sign in & load")
                                        .when(!ready, |d| d.opacity(0.5).cursor_default())
                                        .on_press(cx.listener(move |this, _, _, cx| {
                                            if ready {
                                                this.load_sso(cx)
                                            }
                                        }))
                                        .a11y_disabled(!ready),
                                ),
                        ),
                )
                .children(self.import_tail(p, cx))
            }
            Step::Ai => {
                let url_ok = !self.api_url.read(cx).value().trim().is_empty();
                let key_ok = !self.api_key.read(cx).value().trim().is_empty() || self.api_key_saved;
                let can_load = !self.busy && url_ok && key_ok;
                let has_models = !self.models.is_empty();
                body.child(header(
                    "AI assistant (optional)",
                    Some("Connect an Open WebUI instance to get AWS CLI help inside the terminal. Skip this if you do not have one — everything else works without it.".into()),
                    p,
                ))
                .child(
                    div()
                        .max_w(px(640.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(labelled("API URL", Input::new(&self.api_url), p))
                        .child(labelled("API key", Input::new(&self.api_key).mask_toggle(), p))
                        .child(
                            div().flex().child(
                                secondary_button("wizard-load-models", if self.busy { "Checking…" } else { "Save & load models" }, None, p)
                                    .aria_label("Save & load models")
                                    .when(!can_load, |d| d.opacity(0.5).cursor_default())
                                    .on_press(cx.listener(move |this, _, _, cx| {
                                        if can_load {
                                            this.load_models(cx)
                                        }
                                    }))
                                    .a11y_disabled(!can_load),
                            ),
                        )
                        .when(has_models, |d| {
                            d.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("Model"))
                                    .child(self.model_picker.clone()),
                            )
                        }),
                )
                .children(self.error_line(p))
                .child(self.footer(
                    has_models.then_some(("Save".into(), self.ai_model.is_some(), |t: &mut Self, _: &mut Window, cx: &mut Context<Self>| t.save_model(cx))),
                    Some(("Skip".into(), |t, _, cx| t.skip(cx))),
                    true,
                    p,
                    cx,
                ))
            }
            Step::Done => {
                let plural = |n: usize| if n == 1 { "" } else { "s" };
                let saml = self.created.iter().filter(|p| p.get("authType").and_then(Value::as_str) != Some("identityCenter")).count();
                let sso = self.created.len() - saml;
                let mut lines: Vec<String> = Vec::new();
                if saml > 0 {
                    lines.push(format!("{saml} SAML profile{}", plural(saml)));
                }
                if sso > 0 {
                    lines.push(format!("{sso} Identity Center profile{}", plural(sso)));
                }
                if self.master_set && !self.master_already_set {
                    lines.push("Master password set".into());
                }
                if self.creds_saved {
                    lines.push("IdP sign-in saved".into());
                }
                let list_empty = lines.is_empty();
                let mut list = div().max_w(px(448.)).flex().flex_col().gap(px(8.));
                for (i, l) in lines.into_iter().enumerate() {
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .px(px(16.))
                            .py(px(12.))
                            .rounded(px(size::RADIUS_LG))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.panel)
                            .child(icon("check", 16., p.text_muted))
                            .child(read_text(SharedString::from(format!("done-line-{i}")), l).text_size(px(size::TEXT_SM))),
                    );
                }
                let any = !self.created.is_empty();
                let mut made = div().mt(px(16.)).max_w(px(448.)).flex().flex_col().gap(px(4.));
                for (i, prof) in self.created.iter().take(8).enumerate() {
                    let name = prof.get("name").and_then(Value::as_str).unwrap_or("");
                    let section = prof.get("credentialProfileName").and_then(Value::as_str).unwrap_or("");
                    made = made.child(
                        read_text(SharedString::from(format!("done-made-{i}")), format!("{name} → --profile {section}"))
                            .truncate()
                            .text_size(px(size::TEXT_XS))
                            .text_color(p.text_muted),
                    );
                }
                if self.created.len() > 8 {
                    made = made
                        .child(read_text("done-more", format!("…and {} more", self.created.len() - 8)).text_size(px(size::TEXT_XS)).text_color(p.text_muted));
                }
                let finish = if self.mode == WizardMode::FirstRun { "Finish" } else { "Done" };
                // 1.x: "Use the cloud button"; 2.0's console button shows an app window.
                let (title, blurb) = if any {
                    ("All set", "Credentials refresh automatically in the background. Use the console button on any profile to open the AWS console for it.")
                } else {
                    ("Nothing added yet", "You can add accounts any time from the Add profile menu on the Profiles page.")
                };
                body.child(header(title, Some(blurb.into()), p)).when(!list_empty, |d| d.child(list)).when(any, |d| d.child(made)).child(self.footer(
                    Some((finish.into(), true, |t, _, cx| t.exit(cx))),
                    None,
                    true,
                    p,
                    cx,
                ))
            }
        }
    }
}

fn picker_of(w: &mut WizardView) -> Option<&mut PickerState> {
    w.picker.as_mut()
}

impl Render for WizardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.entered != Some(self.current) {
            self.entered = Some(self.current);
            self.on_enter(self.current, window, cx);
        }
        let p = self.model.read(cx).surface_palette();
        let (title, close) = match self.mode {
            WizardMode::FirstRun => ("Set up", "Skip setup"),
            WizardMode::Import => ("Add accounts", "Close"),
        };
        let steps = self.steps();
        let index = steps.iter().position(|s| *s == self.current).unwrap_or(0);

        let mut list = div().id("wizard-steps").role(Role::List).aria_label("Steps").mt(px(24.)).flex().flex_col().gap(px(4.));
        for (i, s) in steps.iter().enumerate() {
            let (title, ic) = meta(*s);
            let done = i < index;
            let active = i == index;
            let ic = if done {
                if self.skipped.contains(s) {
                    "minus"
                } else {
                    "check"
                }
            } else {
                ic
            };
            let color = if active {
                p.text
            } else if done {
                p.text_muted
            } else {
                p.text_faint
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("wizard-step-{i}")))
                    .role(Role::ListItem)
                    .aria_label(SharedString::from(if active { format!("{title}, current step") } else { title.to_string() }))
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(size::RADIUS))
                    .when(active, |d| d.bg(p.raised))
                    .text_size(px(size::TEXT_SM))
                    .text_color(color)
                    .child(
                        div()
                            .size(px(24.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(if active {
                                p.border_strong
                            } else if done {
                                gpui::transparent_black().into()
                            } else {
                                p.border
                            })
                            .when(done, |d| d.bg(p.raised))
                            .child(icon(ic, 14., color)),
                    )
                    .child(title),
            );
        }

        let rail = div()
            .w(px(256.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .p(px(24.))
            .bg(p.panel)
            .border_r_1()
            .border_color(p.border)
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(list)
            .child(div().flex_1())
            .child(
                div().pt(px(24.)).child(
                    div()
                        .id("wizard-skip-setup")
                        .role(Role::Button)
                        .aria_label(close)
                        .focusable()
                        .tab_stop(true)
                        .focus_ring(&p)
                        .text_size(px(size::TEXT_XS))
                        .text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.text))
                        .on_press(cx.listener(|this, _, _, cx| this.exit(cx)))
                        .child(close),
                ),
            );

        let main = div().id("wizard-main").flex_1().min_w(px(0.)).h_full().overflow_y_scroll().p(px(32.)).child(self.render_step(&p, cx));

        div()
            .id("setup-wizard")
            .role(Role::Dialog)
            .aria_label(title)
            .absolute()
            .inset_0()
            .bg(p.bg)
            .text_color(p.text)
            .flex()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(rail)
            .child(main)
    }
}
