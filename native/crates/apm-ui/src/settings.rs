//! The Settings screen (1.x `pages/Settings.tsx`): tabs for General, Appearance, Credentials,
//! Terminal & AI and Advanced, plus its dialogs (create a master password before saving default
//! credentials, forget credentials, restore a backup).
//!
//! Every field saves as soon as it changes, as 1.x did on change or blur. Settings are saved as a
//! patch on the stored object (`AppModel::save_settings_patch`) so nothing the core wrote in the
//! meantime is lost. Accessible names match the 1.x labels: the shared UI flows find controls by them.
//!
//! Left out on purpose (2.0 has no equivalent yet): "Open developer tools", "Start feature tour",
//! "Add accounts…" (the import wizard), and the background blur slider.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use gpui::{
    anchored, deferred, div, img, point, prelude::*, px, relative, AnchoredPositionMode, Context, ElementId, Entity, FocusHandle,
    FontWeight, Image, ImageFormat, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, Role, SharedString, Stateful, Subscription,
    Task, Window,
};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::{json, Map, Value};

use crate::a11y::{ActivateExt, A11yDisabledExt};
use crate::model::AppModel;
use crate::picker::{Picker, PickerEvent, PickerOption};
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{icon, icon_button, primary_button, secondary_button};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Credentials,
    Terminal,
    Advanced,
}

/// Display order, as 1.x `SETTINGS_TABS`.
const TABS: [(Tab, &str, &str); 5] = [
    (Tab::General, "settings-tab-general", "General"),
    (Tab::Appearance, "settings-tab-appearance", "Appearance"),
    (Tab::Credentials, "settings-tab-credentials", "Credentials"),
    (Tab::Terminal, "settings-tab-terminal", "Terminal & AI"),
    (Tab::Advanced, "settings-tab-advanced", "Advanced"),
];

/// 1.x `BACKGROUND_SCOPES`.
const BACKGROUND_SCOPES: [(&str, &str, &str); 3] = [
    ("off", "Off", "Plain background everywhere."),
    ("auth", "Sign-in screen only", "The rest of the app stays as it is."),
    ("app", "Sign-in screen and app", "Also plays behind the whole window, with translucent panels over it."),
];

const CLASSIC_APP_ICON: &str = "classic";
const HOLD: Duration = Duration::from_millis(1000);

/// A click handler for `on_activate` that updates this view.
fn act(cx: &Context<SettingsView>, f: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
    let me = cx.entity().downgrade();
    move |window, cx| {
        let _ = me.update(cx, |this, cx| f(this, window, cx));
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

struct AppIcon {
    key: String,
    name: String,
    style: String,
    image: Option<Arc<Image>>,
}

/// A line of feedback under a control (1.x's success / error messages).
#[derive(Clone)]
struct Msg {
    error: bool,
    text: String,
}

/// 1.x `CreateMasterPasswordModal`, opened when default credentials are saved before a master
/// password exists.
struct MasterDialog {
    acknowledged: bool,
    /// 0..=1 while the acknowledgement button is held.
    progress: f32,
    hold_started: Option<Instant>,
    hold_task: Option<Task<()>>,
    hold_focus: FocusHandle,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<String>,
    submitting: bool,
    _subs: Vec<Subscription>,
}

enum Dialog {
    Master(MasterDialog),
    /// `config:restore` returned a backup to confirm: settings, profiles, folders.
    Restore(Value),
    ForgetDefault,
    ForgetAll,
}

pub struct SettingsView {
    model: Entity<AppModel>,
    tab: Tab,
    loaded: bool,
    // General
    hours: Entity<InputState>,
    idp: Entity<InputState>,
    sso_start: Entity<InputState>,
    sso_region: Entity<InputState>,
    org_name: Entity<InputState>,
    sso_window: Entity<Picker>,
    console_window: Entity<Picker>,
    browser: Entity<Picker>,
    // Credentials
    username: Entity<InputState>,
    password: Entity<InputState>,
    new_account_id: Entity<InputState>,
    new_account_name: Entity<InputState>,
    /// Account display names: account id, its name field.
    names: Vec<(String, Entity<InputState>)>,
    names_subs: Vec<Subscription>,
    // Terminal & AI
    bash_path: Entity<InputState>,
    terminal_profile: Entity<Picker>,
    api_url: Entity<InputState>,
    api_key: Entity<InputState>,
    ai_model: Entity<Picker>,
    /// The last value saved or loaded per text field, so observing a field saves real edits only
    /// (an input is observed rather than subscribed to: an accessible SetValue sends no Change event).
    last: HashMap<&'static str, String>,
    default_creds: Value,
    master_enabled: bool,
    paused: bool,
    version: String,
    update_status: Value,
    checking_updates: bool,
    browsers: Vec<(String, String)>,
    icons: Vec<AppIcon>,
    icons_requested: bool,
    models: Vec<String>,
    models_loading: bool,
    models_requested: bool,
    models_error: Option<String>,
    msgs: HashMap<&'static str, Msg>,
    background_busy: bool,
    version_taps: (u32, Option<Instant>),
    dev_unlocked: bool,
    dialog: Option<Dialog>,
    _subs: Vec<Subscription>,
}

/// Text fields that are settings keys.
const TEXT_KEYS: [&str; 7] = [
    "defaultSessionDurationHours",
    "defaultIdpEntryUrl",
    "defaultSsoStartUrl",
    "defaultSsoRegion",
    "bashPath",
    "openWebUiApiUrl",
    "openWebUiApiKey",
];

impl SettingsView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let masked = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(placeholder))
        };
        let hours = text("1", window, cx);
        let idp = text("https://adfs.example.com/adfs/ls/...", window, cx);
        let sso_start = text("https://d-xxxxxxxxxx.awsapps.com/start", window, cx);
        let sso_region = text("us-west-2", window, cx);
        let org_name = text("Organization name (optional)", window, cx);
        let username = text("e.g. you@company.com", window, cx);
        let password = masked("Leave blank to keep existing", window, cx);
        let new_account_id = text("Account ID", window, cx);
        let new_account_name = text("Display name", window, cx);
        let bash_path = text(if cfg!(windows) { "C:\\Program Files\\Git\\bin\\bash.exe" } else { "/bin/bash" }, window, cx);
        let api_url = text("https://your-openwebui-instance.com/api", window, cx);
        let api_key = masked("Your Open WebUI API key", window, cx);

        let picker = |id: &'static str, label: &'static str, options: Vec<(&str, &str)>, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut p = Picker::new(id, label, cx);
                p.set_options(options.into_iter().map(|(v, l)| PickerOption { value: v.into(), label: SharedString::from(l.to_string()) }).collect(), cx);
                p
            })
        };
        let sso_window = picker("sso-window", "Identity Center sign-in window", vec![("embedded", "In-app window (recommended)"), ("external", "Default browser")], cx);
        let console_window = picker("console-window", "AWS console window", vec![("external", "Web browser"), ("embedded", "In-app window per profile")], cx);
        let browser = picker("console-browser", "Browser", vec![("default", "System default")], cx);
        let terminal_profile = picker("default-terminal-profile", "Default profile", vec![("", "No profile")], cx);
        let ai_model = cx.new(|cx| {
            let mut p = Picker::new("openwebui-model", "Model", cx);
            p.set_placeholder("Select model", cx);
            p
        });

        let mut subs = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        for (key, input) in TEXT_KEYS.iter().zip([&hours, &idp, &sso_start, &sso_region, &bash_path, &api_url, &api_key]) {
            let key: &'static str = key;
            subs.push(cx.observe(input, move |this, input, cx| {
                let v = input.read(cx).value().to_string();
                this.text_edited(key, v, cx);
            }));
        }
        for input in [&org_name, &username, &password, &new_account_id, &new_account_name] {
            subs.push(cx.observe(input, |this, _, cx| {
                this.msgs.remove("creds");
                cx.notify()
            }));
        }
        for input in [&new_account_id, &new_account_name] {
            subs.push(cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.add_account_name(window, cx);
                }
            }));
        }
        for (key, p) in [("ssoBrowserMode", &sso_window), ("consoleBrowserMode", &console_window), ("consoleBrowser", &browser), ("defaultTerminalProfileId", &terminal_profile), ("openWebUiModel", &ai_model)] {
            subs.push(cx.subscribe(p, move |this, _, ev: &PickerEvent, cx| {
                let PickerEvent::Changed(v) = ev;
                this.save(key, json!(v.clone().unwrap_or_default()), cx);
            }));
        }

        let mut this = SettingsView {
            model,
            tab: Tab::General,
            loaded: false,
            hours,
            idp,
            sso_start,
            sso_region,
            org_name,
            sso_window,
            console_window,
            browser,
            username,
            password,
            new_account_id,
            new_account_name,
            names: Vec::new(),
            names_subs: Vec::new(),
            bash_path,
            terminal_profile,
            api_url,
            api_key,
            ai_model,
            last: HashMap::new(),
            default_creds: Value::Null,
            master_enabled: false,
            paused: false,
            version: String::new(),
            update_status: Value::Null,
            checking_updates: false,
            browsers: Vec::new(),
            icons: Vec::new(),
            icons_requested: false,
            models: Vec::new(),
            models_loading: false,
            models_requested: false,
            models_error: None,
            msgs: HashMap::new(),
            background_busy: false,
            version_taps: (0, None),
            dev_unlocked: false,
            dialog: None,
            _subs: subs,
        };
        this.refresh(window, cx);
        this
    }

    fn bridge(&self, cx: &Context<Self>) -> crate::bridge::Bridge {
        self.model.read(cx).bridge.clone()
    }

    fn settings(&self, cx: &Context<Self>) -> Value {
        self.model.read(cx).settings.clone()
    }

    /// Re-read what the screen shows. 1.x did this each time the page became visible, so values
    /// written elsewhere (the profile form, the tray, the wizard) show up.
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let settings = b.call("settings:get", vec![]).await.unwrap_or(Value::Null);
            let creds = b.call("credentials:getDefaultDisplay", vec![]).await.unwrap_or(Value::Null);
            let master = b.call("credentials:getMasterPasswordEnabled", vec![]).await.ok().and_then(|v| v.as_bool()).unwrap_or(false);
            let paused = b.call("scheduler:getPaused", vec![]).await.ok().and_then(|v| v.get("paused").and_then(Value::as_bool)).unwrap_or(false);
            let version = b.call("app:getVersion", vec![]).await.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let update = b.call("update:getStatus", vec![]).await.unwrap_or(Value::Null);
            let browsers = b.call("system:listBrowsers", vec![]).await.unwrap_or(Value::Null);
            let profiles = b.call("profiles:getAll", vec![]).await.unwrap_or(Value::Null);
            let _ = this.update_in(cx, |this, window, cx| {
                this.default_creds = creds;
                this.master_enabled = master;
                this.paused = paused;
                this.version = version;
                if !update.is_null() {
                    this.update_status = update;
                }
                this.browsers = browsers
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| (s(b, "key").to_string(), s(b, "name").to_string()))
                    .filter(|(k, _)| !k.is_empty())
                    .collect();
                // Never prefilled: the password is not returned to the window, by design.
                let locked = this.default_creds.get("locked") == Some(&Value::Bool(true));
                let user = if locked { String::new() } else { s(&this.default_creds, "username").to_string() };
                this.username.update(cx, |st, cx| st.set_value(user, window, cx));
                this.password.update(cx, |st, cx| st.set_value("", window, cx));
                if settings.is_object() {
                    this.model.update(cx, |m, cx| {
                        m.settings = settings.clone();
                        m.apply_palette(cx);
                        cx.notify();
                    });
                }
                this.apply_settings(&settings, &profiles, window, cx);
                this.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// Fill the fields from stored settings.
    fn apply_settings(&mut self, st: &Value, profiles: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let hours = match st.get("defaultSessionDurationHours") {
            Some(Value::Number(n)) => n.to_string(),
            _ => "1".into(),
        };
        let values: [(&'static str, String, Entity<InputState>); 7] = [
            ("defaultSessionDurationHours", hours, self.hours.clone()),
            ("defaultIdpEntryUrl", s(st, "defaultIdpEntryUrl").into(), self.idp.clone()),
            ("defaultSsoStartUrl", s(st, "defaultSsoStartUrl").into(), self.sso_start.clone()),
            ("defaultSsoRegion", s(st, "defaultSsoRegion").into(), self.sso_region.clone()),
            ("bashPath", s(st, "bashPath").into(), self.bash_path.clone()),
            ("openWebUiApiUrl", s(st, "openWebUiApiUrl").into(), self.api_url.clone()),
            // The saved key never reaches the window: empty means "keep the saved key".
            ("openWebUiApiKey", String::new(), self.api_key.clone()),
        ];
        for (key, v, input) in values {
            self.last.insert(key, v.clone());
            if input.read(cx).value() != v.as_str() {
                input.update(cx, |i, cx| i.set_value(v, window, cx));
            }
        }
        let pick = |p: &Entity<Picker>, v: String, cx: &mut Context<Self>| p.update(cx, |p, cx| p.set_selected(Some(v), cx));
        pick(&self.sso_window, st.get("ssoBrowserMode").and_then(Value::as_str).unwrap_or("embedded").into(), cx);
        pick(&self.console_window, st.get("consoleBrowserMode").and_then(Value::as_str).unwrap_or("external").into(), cx);
        let mut browser_opts = vec![PickerOption { value: "default".into(), label: "System default".into() }];
        browser_opts.extend(self.browsers.iter().map(|(k, n)| PickerOption { value: k.clone(), label: n.clone().into() }));
        let browser = st.get("consoleBrowser").and_then(Value::as_str).unwrap_or("default").to_string();
        self.browser.update(cx, |p, cx| {
            p.set_options(browser_opts, cx);
            p.set_selected(Some(browser), cx);
        });

        // Terminal default profile: show the resolved value, as the Terminal screen will pick it.
        let list: Vec<&Value> = profiles.as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let ids: Vec<&str> = list.iter().map(|p| s(p, "id")).collect();
        let resolved = apm_core::terminal_profile::resolve_terminal_profile_id(st.get("defaultTerminalProfileId").and_then(Value::as_str), &ids)
            .unwrap_or("")
            .to_string();
        let mut opts = vec![PickerOption { value: String::new(), label: "No profile".into() }];
        opts.extend(list.iter().map(|p| PickerOption { value: s(p, "id").into(), label: s(p, "name").to_string().into() }));
        self.terminal_profile.update(cx, |p, cx| {
            p.set_options(opts, cx);
            p.set_selected(Some(resolved), cx);
        });
        self.sync_models(st, cx);
        self.rebuild_names(st, window, cx);
    }

    fn sync_models(&mut self, st: &Value, cx: &mut Context<Self>) {
        let current = s(st, "openWebUiModel").to_string();
        let mut opts: Vec<PickerOption> = Vec::new();
        if !partially_configured(st) {
            opts.push(PickerOption { value: String::new(), label: "(None)".into() });
        }
        if !current.is_empty() && !self.models.contains(&current) {
            opts.push(PickerOption { value: current.clone(), label: current.clone().into() });
        }
        opts.extend(self.models.iter().map(|m| PickerOption { value: m.clone(), label: m.clone().into() }));
        self.ai_model.update(cx, |p, cx| {
            p.set_options(opts, cx);
            p.set_selected((!current.is_empty()).then_some(current), cx);
        });
    }

    /// One name field per mapped account, rebuilt when the set of accounts changes.
    fn rebuild_names(&mut self, st: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let map = st.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
        let ids: Vec<&String> = map.keys().collect();
        let same = ids.len() == self.names.len() && ids.iter().zip(&self.names).all(|(a, (b, _))| *a == b);
        if same {
            return;
        }
        self.names.clear();
        self.names_subs.clear();
        for (id, name) in map {
            let name = name.as_str().unwrap_or("").to_string();
            let input = cx.new(|cx| {
                let mut st = InputState::new(window, cx).placeholder("Display name");
                st.set_value(name.clone(), window, cx);
                st
            });
            let mut last = name;
            let account = id.clone();
            self.names_subs.push(cx.observe(&input, move |this, input, cx| {
                let v = input.read(cx).value().to_string();
                if v == last {
                    return;
                }
                last = v.clone();
                let mut names = this.settings(cx).get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
                names.insert(account.clone(), json!(v));
                this.save("accountDisplayNames", Value::Object(names), cx);
            }));
            self.names.push((id, input));
        }
    }

    fn text_edited(&mut self, key: &'static str, v: String, cx: &mut Context<Self>) {
        if self.last.get(key) == Some(&v) {
            return;
        }
        self.last.insert(key, v.clone());
        if !self.loaded {
            return;
        }
        let value = match key {
            // 1.x: a number field, 1 to 12 hours. Wait for a number while the field is being edited.
            "defaultSessionDurationHours" => match v.trim().parse::<i64>() {
                Ok(n) => json!(n.clamp(1, 12)),
                Err(_) => return,
            },
            // An emptied field keeps the saved key; only "Remove saved key" deletes it.
            "openWebUiApiKey" if v.is_empty() => return,
            _ => json!(v),
        };
        if matches!(key, "openWebUiApiUrl" | "openWebUiApiKey") {
            self.models_requested = false;
        }
        self.save(key, value, cx);
    }

    fn save(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        let mut patch = Map::new();
        patch.insert(key.to_string(), value);
        self.save_patch(patch, cx);
    }

    fn save_patch(&mut self, patch: Map<String, Value>, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| m.save_settings_patch(patch, cx));
        cx.notify();
    }

    fn set_msg(&mut self, key: &'static str, error: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        self.msgs.insert(key, Msg { error, text: text.into() });
        cx.notify();
    }

    /// Run a channel and hand its result to `then` on the UI thread.
    fn call(&mut self, channel: &'static str, args: Vec<Value>, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, Result<Value, String>, &mut Context<Self>) + 'static) {
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call(channel, args).await;
            let _ = this.update(cx, |this, cx| {
                then(this, r, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // General
    // ---------------------------------------------------------------------------------------------

    fn version_tap(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let (count, last) = self.version_taps;
        let count = if last.is_some_and(|t| now.duration_since(t) < Duration::from_millis(2500)) { count + 1 } else { 1 };
        self.version_taps = (count, Some(now));
        if count >= 7 {
            self.version_taps = (0, None);
            self.dev_unlocked = true;
            // What it reveals is on another tab: go there, or the unlock reads as broken.
            self.tab = Tab::Advanced;
            cx.notify();
        }
    }

    fn check_updates(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("update");
        self.checking_updates = true;
        cx.notify();
        let was_error = s(&self.update_status, "type") == "error";
        self.call("update:checkNow", vec![], cx, move |this, r, cx| {
            this.checking_updates = false;
            match r {
                Ok(v) => {
                    let text = match s(&v, "type") {
                        "no-update" => "You're up to date.".to_string(),
                        "available" => format!("Update available: v{}", s(&v, "version")),
                        "error" => format!("Update check failed: {}", v.get("message").and_then(Value::as_str).unwrap_or("Unknown error")),
                        _ => String::new(),
                    };
                    if !was_error && !text.is_empty() {
                        this.set_msg("update", false, text, cx);
                    }
                    this.update_status = v;
                }
                Err(_) => this.set_msg("update", true, "Update check failed.", cx),
            }
        });
    }

    fn export_org(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("org");
        let name = self.org_name.read(cx).value().trim().to_string();
        let arg = if name.is_empty() { Value::Null } else { json!(name) };
        self.call("orgConfig:export", vec![arg], cx, |this, r, cx| match r {
            Ok(v) if v.get("canceled").is_some() => {}
            Ok(v) if v.get("success") == Some(&Value::Bool(true)) => this.set_msg("org", false, format!("Exported to {}", s(&v, "path")), cx),
            Ok(v) => this.set_msg("org", true, s(&v, "error").to_string(), cx),
            Err(e) => this.set_msg("org", true, e, cx),
        });
    }

    fn import_org(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("org");
        let b = self.bridge(cx);
        cx.spawn(async move |this, cx| {
            let r = b.call("orgConfig:import", vec![]).await;
            let outcome: Option<Msg> = match r {
                Ok(v) if v.get("canceled").is_some() => None,
                Ok(v) if v.get("success") == Some(&Value::Bool(true)) => {
                    // Re-validate after the hop rather than trusting the shape that arrives.
                    match apm_core::org_config::parse(v.get("config").unwrap_or(&Value::Null)).filter(|c| !apm_core::org_config::is_empty(c)) {
                        None => Some(Msg { error: true, text: "That file did not contain any settings this app can use.".into() }),
                        Some(config) => {
                            let current = b.call("settings:get", vec![]).await.ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
                            let merged = apm_core::org_config::apply_to_settings(&current, &config, chrono::Utc::now());
                            match b.call("settings:save", vec![Value::Object(merged)]).await {
                                Ok(_) => Some(Msg {
                                    error: false,
                                    text: format!("Applied {}. Existing values were kept.", apm_core::org_config::describe(&config).join(", ")),
                                }),
                                Err(e) => Some(Msg { error: true, text: e }),
                            }
                        }
                    }
                }
                Ok(v) => Some(Msg { error: true, text: s(&v, "error").to_string() }),
                Err(e) => Some(Msg { error: true, text: e }),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(m) = outcome {
                    let ok = !m.error;
                    this.msgs.insert("org", m);
                    if ok {
                        this.refresh(window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn enable_multi_session(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("multi");
        self.call("console:enableMultiSession", vec![], cx, |this, r, cx| match r {
            Ok(v) if v.get("success") == Some(&Value::Bool(true)) => this.set_msg(
                "multi",
                false,
                "Opened AWS in your browser. If it shows an error, multi-account sign-in is already enabled — nothing more to do.",
                cx,
            ),
            Ok(v) => this.set_msg("multi", true, s(&v, "error").to_string(), cx),
            Err(e) => this.set_msg("multi", true, e, cx),
        });
    }

    fn toggle_paused(&mut self, cx: &mut Context<Self>) {
        let next = !self.paused;
        self.call("scheduler:setPaused", vec![json!(next)], cx, move |this, r, cx| {
            if r.is_ok() {
                this.paused = next;
            }
            this.model.update(cx, |m, cx| m.reload(cx));
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Credentials
    // ---------------------------------------------------------------------------------------------

    fn creds_locked(&self) -> bool {
        self.default_creds.get("locked") == Some(&Value::Bool(true))
    }

    fn reload_creds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let d = b.call("credentials:getDefaultDisplay", vec![]).await.unwrap_or(Value::Null);
            let master = b.call("credentials:getMasterPasswordEnabled", vec![]).await.ok().and_then(|v| v.as_bool()).unwrap_or(false);
            let _ = this.update_in(cx, |this, window, cx| {
                let user = if d.get("locked") == Some(&Value::Bool(true)) { String::new() } else { s(&d, "username").to_string() };
                this.default_creds = d;
                this.master_enabled = master;
                this.username.update(cx, |st, cx| st.set_value(user, window, cx));
                this.password.update(cx, |st, cx| st.set_value("", window, cx));
                this.msgs.remove("creds");
                cx.notify();
            });
        })
        .detach();
    }

    fn save_default_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.msgs.remove("creds");
        let username = self.username.read(cx).value().trim().to_string();
        if username.is_empty() {
            self.set_msg("creds", true, "Username is required.", cx);
            return;
        }
        let password = self.password.read(cx).value().to_string();
        let pw = if password.is_empty() { Value::Null } else { json!(password) };
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let r = b.call("credentials:setDefault", vec![json!(username), pw]).await;
            let _ = this.update_in(cx, |this, window, cx| match r {
                Ok(v) if v.get("success") == Some(&Value::Bool(false)) && s(&v, "error") == "MASTER_PASSWORD_REQUIRED" => {
                    this.open_master_dialog(window, cx);
                }
                Ok(v) if v.get("success") == Some(&Value::Bool(false)) => this.set_msg("creds", true, s(&v, "error").to_string(), cx),
                Err(e) => this.set_msg("creds", true, e, cx),
                Ok(_) => this.reload_creds(window, cx),
            });
        })
        .detach();
    }

    fn open_master_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Choose a password"));
        let confirm = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Re-enter password"));
        let subs = vec![
            cx.observe(&password, |_, _, cx| cx.notify()),
            cx.observe(&confirm, |_, _, cx| cx.notify()),
            cx.subscribe_in(&confirm, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.create_master_and_save(window, cx);
                }
            }),
        ];
        let hold_focus = cx.focus_handle();
        hold_focus.focus(window, cx);
        self.dialog = Some(Dialog::Master(MasterDialog {
            acknowledged: false,
            progress: 0.,
            hold_started: None,
            hold_task: None,
            hold_focus,
            password,
            confirm,
            error: None,
            submitting: false,
            _subs: subs,
        }));
        cx.notify();
    }

    /// Press and hold for one second (pointer, Space or Enter) to acknowledge, as 1.x.
    fn hold_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Master(d)) = &mut self.dialog else { return };
        if d.acknowledged || d.hold_started.is_some() {
            return;
        }
        d.hold_started = Some(Instant::now());
        d.hold_task = Some(cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            let done = this
                .update_in(cx, |this, window, cx| {
                    let Some(Dialog::Master(d)) = &mut this.dialog else { return true };
                    let Some(start) = d.hold_started else { return true };
                    d.progress = (start.elapsed().as_secs_f32() / HOLD.as_secs_f32()).min(1.);
                    if d.progress >= 1. {
                        d.acknowledged = true;
                        d.hold_started = None;
                        d.progress = 0.;
                        d.password.update(cx, |st, cx| st.focus(window, cx));
                    }
                    cx.notify();
                    d.acknowledged
                })
                .unwrap_or(true);
            if done {
                break;
            }
        }));
        cx.notify();
    }

    fn hold_end(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Master(d)) = &mut self.dialog else { return };
        if d.acknowledged {
            return;
        }
        d.hold_started = None;
        d.hold_task = None;
        d.progress = 0.;
        cx.notify();
    }

    fn create_master_and_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Master(d)) = &mut self.dialog else { return };
        if d.submitting {
            return;
        }
        d.error = None;
        let password = d.password.read(cx).value().to_string();
        let confirm = d.confirm.read(cx).value().to_string();
        if password.trim().is_empty() {
            d.error = Some("Please enter a password.".into());
            cx.notify();
            return;
        }
        if password != confirm {
            // 1.x: the live "Passwords don't match" line already says why.
            cx.notify();
            return;
        }
        if let Some(e) = apm_core::master_password::validate(&password) {
            d.error = Some(e);
            cx.notify();
            return;
        }
        d.submitting = true;
        cx.notify();
        let username = self.username.read(cx).value().to_string();
        let default_pw = self.password.read(cx).value().to_string();
        let default_pw = if default_pw.is_empty() { Value::Null } else { json!(default_pw) };
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let created = b.call("credentials:createMasterPassword", vec![json!(password), json!(confirm)]).await;
            let error = match created {
                Ok(v) if v.get("success") == Some(&Value::Bool(true)) => {
                    let _ = b.call("credentials:setDefault", vec![json!(username), default_pw]).await;
                    None
                }
                Ok(v) => Some(v.get("error").and_then(Value::as_str).unwrap_or("Could not create the master password").to_string()),
                Err(e) => Some(e),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                match error {
                    Some(e) => {
                        if let Some(Dialog::Master(d)) = &mut this.dialog {
                            d.error = Some(e);
                            d.submitting = false;
                        }
                    }
                    None => {
                        this.dialog = None;
                        this.master_enabled = true;
                        this.reload_creds(window, cx);
                        this.model.update(cx, |m, cx| m.reload(cx));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn close_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Master(d)) = &self.dialog {
            if d.submitting {
                return;
            }
        }
        self.dialog = None;
        cx.notify();
    }

    fn forget_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let _ = b.call("credentials:forgetDefault", vec![]).await;
            let _ = this.update_in(cx, |this, window, cx| this.reload_creds(window, cx));
        })
        .detach();
    }

    fn forget_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let _ = b.call("credentials:forgetAllAndResetMasterPassword", vec![]).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.reload_creds(window, cx);
                this.model.update(cx, |m, cx| m.reload(cx));
            });
        })
        .detach();
    }

    fn add_account_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.new_account_id.read(cx).value().trim().to_string();
        if id.is_empty() {
            return;
        }
        let name = self.new_account_name.read(cx).value().trim().to_string();
        let mut st = self.settings(cx);
        let mut names = st.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
        names.insert(id, json!(name));
        if let Some(o) = st.as_object_mut() {
            o.insert("accountDisplayNames".into(), Value::Object(names.clone()));
        }
        self.save("accountDisplayNames", Value::Object(names), cx);
        self.new_account_id.update(cx, |i, cx| i.set_value("", window, cx));
        self.new_account_name.update(cx, |i, cx| i.set_value("", window, cx));
        self.rebuild_names(&st, window, cx);
    }

    fn remove_account_name(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let mut st = self.settings(cx);
        let mut names = st.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
        names.shift_remove(&id);
        if let Some(o) = st.as_object_mut() {
            o.insert("accountDisplayNames".into(), Value::Object(names.clone()));
        }
        self.save("accountDisplayNames", Value::Object(names), cx);
        self.rebuild_names(&st, window, cx);
    }

    fn restore_default_names(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.msgs.remove("names");
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let defaults = b.call("settings:getDefaultAccountDisplayNames", vec![]).await.ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
            let _ = this.update_in(cx, |this, window, cx| {
                if defaults.is_empty() {
                    this.set_msg("names", false, "There are no names to restore.", cx);
                    return;
                }
                let mut st = this.settings(cx);
                if let Some(o) = st.as_object_mut() {
                    o.insert("accountDisplayNames".into(), Value::Object(defaults.clone()));
                }
                this.save("accountDisplayNames", Value::Object(defaults), cx);
                this.names.clear();
                this.rebuild_names(&st, window, cx);
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Appearance
    // ---------------------------------------------------------------------------------------------

    /// The previews are ~20 images: loaded the first time the tab opens, not at start-up (1.x).
    fn load_icons(&mut self, cx: &mut Context<Self>) {
        self.icons_requested = true;
        self.call("app:listIcons", vec![], cx, |this, r, _| {
            let list = r.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            this.icons = list
                .iter()
                .map(|o| AppIcon {
                    key: s(o, "key").into(),
                    name: s(o, "name").into(),
                    style: s(o, "style").into(),
                    image: o
                        .get("dataUrl")
                        .and_then(Value::as_str)
                        .and_then(|u| u.split_once("base64,"))
                        .and_then(|(_, b64)| base64::engine::general_purpose::STANDARD.decode(b64).ok())
                        .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes))),
                })
                .collect();
        });
    }

    /// True when the stored icon is Classic, absent, or a design this build no longer ships.
    fn resolves_to_classic(&self, st: &Value) -> bool {
        let key = s(st, "appIcon");
        key.is_empty() || key == CLASSIC_APP_ICON || (!self.icons.is_empty() && !self.icons.iter().any(|i| i.key == key))
    }

    fn choose_video(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("background");
        self.background_busy = true;
        cx.notify();
        self.call("appearance:chooseVideo", vec![], cx, |this, r, cx| {
            this.background_busy = false;
            match r {
                Ok(v) if v.get("canceled").is_some() => {}
                Ok(v) if v.get("success") == Some(&Value::Bool(false)) => this.set_msg("background", true, s(&v, "error").to_string(), cx),
                Ok(v) => {
                    this.set_msg("background", false, format!("Using {}", s(&v, "fileName")), cx);
                    this.model.update(cx, |m, cx| m.reload(cx));
                }
                Err(e) => this.set_msg("background", true, e, cx),
            }
        });
    }

    fn clear_video(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("background");
        self.background_busy = true;
        cx.notify();
        self.call("appearance:clearVideo", vec![], cx, |this, _, cx| {
            this.background_busy = false;
            this.model.update(cx, |m, cx| m.reload(cx));
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Terminal & AI
    // ---------------------------------------------------------------------------------------------

    fn browse_bash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.bridge(cx);
        cx.spawn_in(window, async move |this, cx| {
            let r = b.call("settings:selectBashPath", vec![]).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok(v) = r {
                    let path = s(&v, "path").to_string();
                    if !path.is_empty() {
                        // The field's observer saves it.
                        this.bash_path.update(cx, |i, cx| i.set_value(path, window, cx));
                    }
                }
            });
        })
        .detach();
    }

    fn fetch_models(&mut self, cx: &mut Context<Self>) {
        self.models_requested = true;
        self.models_loading = true;
        self.models_error = None;
        cx.notify();
        self.call("ai:getModels", vec![], cx, |this, r, cx| {
            this.models_loading = false;
            match r {
                Ok(v) if v.get("error").is_some() => {
                    this.models_error = Some(s(&v, "error").to_string());
                    this.models.clear();
                }
                Ok(v) => {
                    this.models = v.get("models").and_then(Value::as_array).into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
                }
                Err(e) => this.models_error = Some(e),
            }
            let st = this.settings(cx);
            this.sync_models(&st, cx);
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Advanced
    // ---------------------------------------------------------------------------------------------

    fn backup(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("backup");
        self.call("config:backup", vec![], cx, |this, r, cx| match r {
            Ok(v) if v.get("canceled") == Some(&Value::Bool(true)) => {}
            Ok(v) if v.get("success") == Some(&Value::Bool(true)) => this.set_msg("backup", false, format!("Backup saved to {}", s(&v, "path")), cx),
            Ok(v) => this.set_msg("backup", true, v.get("error").and_then(Value::as_str).unwrap_or("Backup failed").to_string(), cx),
            Err(e) => this.set_msg("backup", true, e, cx),
        });
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        self.msgs.remove("backup");
        self.call("config:restore", vec![], cx, |this, r, cx| match r {
            Ok(v) if v.get("canceled") == Some(&Value::Bool(true)) => {}
            Ok(v) if v.get("confirm") == Some(&Value::Bool(true)) => this.dialog = Some(Dialog::Restore(v)),
            Ok(v) => this.set_msg("backup", true, v.get("error").and_then(Value::as_str).unwrap_or("Restore failed").to_string(), cx),
            Err(e) => this.set_msg("backup", true, e, cx),
        });
    }

    fn apply_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Restore(v)) = self.dialog.take() else { return };
        let args = vec![
            v.get("settings").cloned().unwrap_or(json!({})),
            v.get("profiles").cloned().unwrap_or(json!([])),
            // A version 1 backup carries none; restore is replace-all, so folders are cleared.
            v.get("folders").cloned().unwrap_or(json!([])),
        ];
        let b = self.bridge(cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let r = b.call("config:applyRestore", args).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match r {
                    Ok(v) if v.get("success") == Some(&Value::Bool(true)) => this.set_msg("backup", false, "Config restored.", cx),
                    Ok(v) => this.set_msg("backup", true, v.get("error").and_then(Value::as_str).unwrap_or("Restore failed").to_string(), cx),
                    Err(e) => this.set_msg("backup", true, e, cx),
                }
                this.model.update(cx, |m, cx| m.reload(cx));
                this.refresh(window, cx);
            });
        })
        .detach();
    }
}

/// 1.x `apiKeySaved`, for the stored settings (the window never holds the key).
fn api_key_saved(st: &Value) -> bool {
    st.get("openWebUiApiKeySet") == Some(&Value::Bool(true))
}

fn partially_configured(st: &Value) -> bool {
    !s(st, "openWebUiApiUrl").trim().is_empty() || api_key_saved(st)
}

// -------------------------------------------------------------------------------------------------
// Building blocks
// -------------------------------------------------------------------------------------------------

fn section(title: &'static str, subtitle: Option<&'static str>, p: &Palette) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(20.))
        .rounded(px(size::RADIUS_LG))
        .border_1()
        .border_color(p.border)
        .bg(p.panel)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child(title))
                .when_some(subtitle, |d, t| d.child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(t))),
        )
}

fn label(text: impl Into<SharedString>, p: &Palette) -> gpui::Div {
    div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(text.into())
}

fn hint(text: impl Into<SharedString>, p: &Palette) -> gpui::Div {
    div().text_size(px(size::TEXT_XS)).text_color(p.text_faint).child(text.into())
}

/// Label, control, optional hint.
fn field(name: &'static str, control: impl IntoElement, hint_text: Option<&'static str>, p: &Palette) -> gpui::Div {
    div().flex().flex_col().gap(px(6.)).child(label(name, p)).child(control).when_some(hint_text, |d, h| d.child(hint(h, p)))
}

/// A feedback line, readable by assistive technology.
fn message(id: &'static str, m: Option<&Msg>, p: &Palette) -> Option<Stateful<gpui::Div>> {
    m.map(|m| {
        div()
            .id(id)
            .role(if m.error { Role::Alert } else { Role::Status })
            .aria_label(SharedString::from(m.text.clone()))
            .text_size(px(size::TEXT_SM))
            .text_color(if m.error { p.danger } else { p.text_muted })
            .child(m.text.clone())
    })
}

/// A segmented control: a group of toggle buttons named by `label`.
fn segmented(id: &'static str, label: &'static str, p: &Palette) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(label)
        .flex()
        .gap(px(2.))
        .p(px(3.))
        .w_auto()
        .rounded(px(8.))
        .border_1()
        .border_color(p.border)
        .bg(p.sunken)
}

fn segment(id: impl Into<ElementId>, text: &'static str, on: bool, p: &Palette) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(text)
        .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
        .focusable()
        .tab_stop(true)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(size::TEXT_SM))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .border_1()
        .when(on, |d| d.bg(p.raised).border_color(p.border_strong).text_color(p.text))
        .when(!on, |d| d.border_color(gpui::transparent_black()))
        .when(!on, |d| d.text_color(p.text_muted).hover(|d| d.text_color(p.text)))
        .child(text)
}

/// A checkbox that saves one boolean setting.
fn setting_checkbox(id: &'static str, text: &'static str, checked: bool, key: &'static str, cx: &mut Context<SettingsView>) -> Checkbox {
    // The accessible Click is answered directly too (see `ActivateExt`): the box may be scrolled away.
    let on_a11y = act(cx, move |this, _, cx| this.save(key, json!(!checked), cx));
    Checkbox::new(id)
        .label(text)
        .checked(checked)
        .on_click(cx.listener(move |this, value: &bool, _, cx| this.save(key, json!(*value), cx)))
        .on_a11y_action(gpui::AccessibleAction::Click, move |_, window, cx| on_a11y(window, cx))
}

/// A secondary button the height of an input, for a button beside a field.
fn inline_button(id: &'static str, text: &'static str, icon_name: Option<&str>, p: &Palette) -> Stateful<gpui::Div> {
    secondary_button(id, text, icon_name, p).h(px(size::CONTROL)).px(px(12.)).text_size(px(size::TEXT_SM))
}

fn danger_button(id: &'static str, text: &'static str, p: &Palette) -> Stateful<gpui::Div> {
    secondary_button(id, text, None, p).text_color(p.danger)
}

fn dialog_card(id: &'static str, title: &'static str, alert: bool, p: &Palette) -> Stateful<gpui::Div> {
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

fn dialog_title(text: &'static str) -> gpui::Div {
    div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(text)
}

fn dialog_body(text: &'static str, p: &Palette) -> gpui::Div {
    div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(text)
}

// -------------------------------------------------------------------------------------------------
// Render
// -------------------------------------------------------------------------------------------------

impl SettingsView {
    fn render_general(&mut self, st: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let status = s(&self.update_status, "type").to_string();
        let update_button = if status == "downloaded" || status == "installing" {
            let installing = status == "installing";
            let v = s(&self.update_status, "version").to_string();
            let text = if installing {
                "Restarting to update…".to_string()
            } else if v.is_empty() {
                "Install update and restart".to_string()
            } else {
                format!("Install update v{v} and restart")
            };
            secondary_button("install-update", text, Some("download"), p)
                .text_color(p.success)
                .when(!installing, |d| {
                    d.on_activate(act(cx, |this, _, cx| {
                        if let Some(o) = this.update_status.as_object_mut() {
                            o.insert("type".into(), json!("installing"));
                        }
                        this.call("update:installAndRestart", vec![], cx, |_, _, _| {});
                    }))
                })
                .a11y_disabled(installing)
                .into_any_element()
        } else {
            let text = if self.checking_updates {
                "Checking…"
            } else if status == "error" {
                "Retry download"
            } else {
                "Check for updates"
            };
            secondary_button("check-updates", text, Some("refresh-cw"), p)
                .aria_label(if status == "error" { "Retry download" } else { "Check for updates" })
                .on_activate(act(cx, |this, _, cx| {
                    if !this.checking_updates {
                        this.check_updates(cx)
                    }
                }))
                .into_any_element()
        };

        let version = (!self.version.is_empty()).then(|| {
            div()
                .id("app-version")
                .flex()
                .gap(px(4.))
                .text_size(px(size::TEXT_SM))
                .on_activate(act(cx, |this, _, cx| this.version_tap(cx)))
                .child(div().text_color(p.text_muted).child("Version"))
                .child(div().font_family(FONT_MONO).text_color(p.text).child(self.version.clone()))
        });

        let imported = st.get("orgConfigImportedAt").and_then(Value::as_str).filter(|t| !t.is_empty()).map(|at| {
            let when = chrono::DateTime::parse_from_rfc3339(at).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or(at.to_string());
            let name = s(st, "orgConfigImportedName");
            let name = if name.is_empty() { String::new() } else { format!(" “{name}”") };
            format!("Last imported{name} on {when}.")
        });

        let external = st.get("consoleBrowserMode").and_then(Value::as_str).unwrap_or("external") == "external";
        let browser_hint = if !external {
            "Applies when the console opens in a web browser."
        } else if !self.browsers.is_empty() {
            "Detected on this machine. Each browser keeps its own AWS sessions."
        } else {
            "No supported browsers detected; the system default will be used."
        };

        let checkbox = |id: &'static str, text: &'static str, key: &'static str, cx: &mut Context<Self>| {
            setting_checkbox(id, text, st.get(key) == Some(&Value::Bool(true)), key, cx)
        };

        let general = section("General", Some("Version, updates, and startup options"), p)
            .children(version)
            .child(div().flex().items_center().gap(px(12.)).child(update_button).children(message("update-msg", self.msgs.get("update"), p)))
            .child(field(
                "Default session duration (hours)",
                div().w(px(96.)).child(Input::new(&self.hours).aria_label("Default session duration (hours)").role(Role::SpinButton)),
                None,
                p,
            ))
            .child(field(
                "Default IdP entry URL",
                Input::new(&self.idp).aria_label("Default IdP entry URL"),
                Some("Used when creating a new profile; you can change it per profile."),
                p,
            ))
            .child(
                field(
                    "Organization configuration",
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(div().flex_1().child(Input::new(&self.org_name).aria_label("Organization name")))
                        .child(inline_button("org-export", "Export…", Some("upload"), p).on_activate(act(cx, |this, _, cx| this.export_org(cx))))
                        .child(inline_button("org-import", "Import…", Some("download"), p).on_activate(act(cx, |this, _, cx| this.import_org(cx)))),
                    Some(
                        "Share this file with your team so setup is prefilled for them. It contains the IdP and SSO URLs, the AI assistant URL and your account display names — never passwords, API keys or credentials. Importing only fills in blanks — it never overwrites settings you have already set.",
                    ),
                    p,
                )
                .when_some(imported, |d, t| d.child(hint(t, p)))
                .children(message("org-msg", self.msgs.get("org"), p)),
            )
            .child(field(
                "Default SSO start URL",
                Input::new(&self.sso_start).aria_label("Default SSO start URL"),
                Some("For IAM Identity Center profiles. Shown in the access portal under any account, Access keys, “AWS IAM Identity Center credentials”."),
                p,
            ))
            .child(field("Default SSO region", div().w(px(180.)).child(Input::new(&self.sso_region).aria_label("Default SSO region")), None, p))
            .child(field(
                "Identity Center sign-in window",
                div().max_w(px(420.)).child(self.sso_window.clone()),
                Some("The in-app window keeps its own session, so it stays separate from whichever account your normal browser is signed into — and it remembers that session between restarts. Switch to the default browser only if your organization blocks embedded sign-in windows."),
                p,
            ))
            .child(field(
                "AWS console window",
                div().max_w(px(420.)).child(self.console_window.clone()),
                Some("Used by the console button on each profile. Either way several accounts can be signed in at once — AWS multi-session is enabled automatically the first time."),
                p,
            ))
            .child(field("Browser", div().max_w(px(420.)).when(!external, |d| d.opacity(0.6)).child(self.browser.clone()), Some(browser_hint), p))
            .child(
                field(
                    "Multi-account sign-in",
                    div().flex().child(
                        secondary_button("multi-session", "Enable multi-account sign-in…", None, p).on_activate(act(cx, |this, _, cx| this.enable_multi_session(cx))),
                    ),
                    Some("Lets several AWS accounts stay signed in at once in the same browser. Set up automatically the first time you open the console — use this only if opening a second account says you must log out first."),
                    p,
                )
                .children(message("multi-msg", self.msgs.get("multi"), p)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(checkbox("launch-at-startup", "Launch at startup", "launchAtStartup", cx))
                    .child(checkbox("start-minimized", "Start minimized to tray", "startMinimizedToTray", cx)),
            );

        let paused = self.paused;
        let refresh = section("Auto refresh", Some("Pause or resume automatic credential refresh"), p).child(div().flex().child(if paused {
            primary_button("toggle-paused", "Resume auto refresh", None, p).on_activate(act(cx, |this, _, cx| this.toggle_paused(cx)))
        } else {
            secondary_button("toggle-paused", "Pause auto refresh", None, p).on_activate(act(cx, |this, _, cx| this.toggle_paused(cx)))
        }));

        div().flex().flex_col().gap(px(16.)).child(general).child(refresh)
    }

    fn render_appearance(&mut self, st: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        if !self.icons_requested {
            self.load_icons(cx);
        }
        let theme = st.get("theme").and_then(Value::as_str).unwrap_or("system").to_string();
        let style = st.get("appStyle").and_then(Value::as_str).unwrap_or("modern").to_string();
        let layout = st.get("profileListLayout").and_then(Value::as_str).unwrap_or("compact").to_string();

        let mut theme_group = segmented("theme-group", "Theme", p);
        for (value, text) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
            theme_group = theme_group.child(
                segment(SharedString::from(format!("theme-{value}")), text, theme == value, p).on_activate(act(cx, move |this, _, cx| this.save("theme", json!(value), cx))),
            );
        }
        let mut style_group = segmented("style-group", "Style", p);
        for (value, text) in [("modern", "Modern"), ("classic", "Classic")] {
            style_group = style_group.child(
                segment(SharedString::from(format!("style-{value}")), text, style == value, p).on_activate(act(cx, move |this, _, cx| this.save("appStyle", json!(value), cx))),
            );
        }
        let mut layout_group = segmented("layout-group", "Profile list", p);
        for (value, text) in [("compact", "Compact"), ("cards", "Cards")] {
            layout_group = layout_group.child(
                segment(SharedString::from(format!("layout-{value}")), text, layout == value, p)
                    .on_activate(act(cx, move |this, _, cx| this.save("profileListLayout", json!(value), cx))),
            );
        }
        let looks = section("Theme", Some("System follows your Windows or macOS setting"), p)
            .child(div().flex().child(theme_group))
            .child(field("Style", div().flex().child(style_group), Some("Modern is the 2.0 look; Classic keeps the 1.x colors."), p))
            .child(field("Profile list", div().flex().child(layout_group), Some("Compact rows with a details panel, or the 1.x cards."), p));

        // App icon
        let classic = self.resolves_to_classic(st);
        let current = s(st, "appIcon").to_string();
        let mut grid = div().id("app-icon-group").role(Role::Group).aria_label("App icon").flex().flex_wrap().gap(px(8.));
        if self.icons.is_empty() {
            grid = grid.child(hint("Loading icons…", p));
        }
        for (i, option) in self.icons.iter().enumerate() {
            let selected = if option.key == CLASSIC_APP_ICON { classic } else { current == option.key };
            let key = option.key.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("app-icon-{i}")))
                    .role(Role::Button)
                    // Name and style, as the 1.x button read (its text content).
                    .aria_label(SharedString::from(format!("{} {}", option.name, option.style)))
                    .aria_toggled(if selected { gpui::Toggled::True } else { gpui::Toggled::False })
                    .focusable()
                    .tab_stop(true)
                    .w(px(104.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .px(px(8.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(if selected { p.accent } else { p.border })
                    .bg(if selected { p.selected } else { p.sunken })
                    .cursor_pointer()
                    .when(!selected, |d| d.hover(|d| d.border_color(p.border_strong)))
                    .on_activate(act(cx, move |this, _, cx| this.save("appIcon", json!(key.clone()), cx)))
                    .child(match &option.image {
                        Some(image) => img(image.clone()).size(px(48.)).into_any_element(),
                        None => div().size(px(48.)).rounded(px(8.)).bg(p.panel).into_any_element(),
                    })
                    .child(div().mt(px(2.)).text_size(px(size::TEXT_XS)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(option.name.clone()))
                    .child(div().text_size(px(11.)).text_color(p.text_muted).child(option.style.clone())),
            );
        }
        let tray_style = st.get("trayIconStyle").and_then(Value::as_str).unwrap_or("color").to_string();
        let mut tray_group = segmented("tray-style-group", "Tray icon style", p);
        for (value, text) in [("color", "Color"), ("monochrome", "Monochrome")] {
            tray_group = tray_group.child(
                segment(SharedString::from(format!("tray-{value}")), text, tray_style == value, p)
                    .when(classic, |d| d.opacity(0.4).cursor_default())
                    .on_activate(act(cx, move |this, _, cx| {
                        let st = this.settings(cx);
                        if !this.resolves_to_classic(&st) {
                            this.save("trayIconStyle", json!(value), cx)
                        }
                    }))
                    .a11y_disabled(classic),
            );
        }
        let badge = st.get("trayStatusBadge") != Some(&Value::Bool(false));
        let icons = section("App icon", Some("Changes the window, taskbar, Dock, tray and Windows shortcut icons. The installer keeps the Classic icon."), p)
            .child(grid)
            .child(field(
                "Tray icon style",
                div().flex().child(tray_group),
                Some(if classic {
                    "Classic has no monochrome version, so its tray icon is always in color."
                } else {
                    "Monochrome draws just the symbol, in the same color as the system icons next to it."
                }),
                p,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(setting_checkbox("tray-status-badge", "Show credential status on the tray icon", badge, "trayStatusBadge", cx))
                    .child(div().pl(px(24.)).child(hint(
                        "A dot appears when credentials are about to expire (amber), auto refresh stopped after failures (red), or auto refresh is paused (gray).",
                        p,
                    ))),
            );

        // Background video
        let busy = self.background_busy;
        let video_name = s(st, "backgroundVideoName").to_string();
        let scope = st.get("backgroundScope").and_then(Value::as_str).unwrap_or("auth").to_string();
        let mut scopes = div().id("background-scope").role(Role::Group).aria_label("Where it applies").flex().flex_col().gap(px(8.));
        for (key, text, description) in BACKGROUND_SCOPES {
            let on = scope == key;
            scopes = scopes.child(
                div()
                    .id(SharedString::from(format!("scope-{key}")))
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("{text} {description}")))
                    .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                    .focusable()
                    .tab_stop(true)
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(if on { p.accent } else { p.border })
                    .bg(if on { p.selected } else { p.sunken })
                    .cursor_pointer()
                    .on_activate(act(cx, move |this, _, cx| this.save("backgroundScope", json!(key), cx)))
                    .child(
                        div()
                            .mt(px(2.))
                            .size(px(16.))
                            .flex_shrink_0()
                            .rounded_full()
                            .border_2()
                            .border_color(if on { p.accent } else { p.border_strong })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(on, |d| d.child(div().size(px(8.)).rounded_full().bg(p.accent))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(text))
                            .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(description)),
                    ),
            );
        }
        let background = section("Background", Some("Play a looping video behind the app instead of the plain background"), p)
            .child(
                field(
                    "Video",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            primary_button("choose-video", if busy { "Working…" } else { "Choose video…" }, Some("film"), p)
                                .aria_label("Choose video…")
                                .on_activate(act(cx, |this, _, cx| {
                                    if !this.background_busy {
                                        this.choose_video(cx)
                                    }
                                }))
                                .a11y_disabled(busy),
                        )
                        .when(!video_name.is_empty(), |d| {
                            d.child(div().max_w(px(280.)).truncate().text_size(px(size::TEXT_SM)).child(video_name.clone()))
                                .child(secondary_button("clear-video", "Remove", None, p).on_activate(act(cx, |this, _, cx| {
                                    if !this.background_busy {
                                        this.clear_video(cx)
                                    }
                                })))
                        }),
                    Some("MP4 or WebM."),
                    p,
                )
                .when(video_name.is_empty(), |d| d.child(hint("No video chosen", p)))
                .children(message("background-msg", self.msgs.get("background"), p)),
            )
            .child(field("Where it applies", scopes, None, p));

        div().flex().flex_col().gap(px(16.)).child(looks).child(icons).child(background)
    }

    fn render_credentials(&mut self, st: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let locked = self.creds_locked();
        let has_saved = !self.default_creds.is_null() && !locked && (!s(&self.default_creds, "username").is_empty() || self.default_creds.get("hasPassword") == Some(&Value::Bool(true)));
        let defaults = section("Default credentials", None, p)
            .child(div().max_w(px(640.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(
                "Profiles set to use default credentials refresh without asking you to sign in. Save a username alone, or a username and password. A blank password keeps the saved one. Stored encrypted in the OS credential store.",
            ))
            .when(locked, |d| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(p.border)
                        .bg(p.sunken)
                        .text_size(px(size::TEXT_SM))
                        .text_color(p.text_muted)
                        .child("Saved credentials are locked. Enter your master password when you start the app to unlock and view or edit."),
                )
            })
            .child(field("Username", div().max_w(px(320.)).child(Input::new(&self.username).aria_label("Username").disabled(locked)), None, p))
            .child(field(
                "Password (optional)",
                div().max_w(px(320.)).child(Input::new(&self.password).aria_label("Password (optional)").mask_toggle().disabled(locked)),
                None,
                p,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        primary_button("save-default-creds", "Save default credentials", None, p)
                            .when(locked, |d| d.opacity(0.5).cursor_default())
                            .on_activate(act(cx, |this, window, cx| {
                                if !this.creds_locked() {
                                    this.save_default_credentials(window, cx)
                                }
                            }))
                            .a11y_disabled(locked),
                    )
                    .when(has_saved, |d| {
                        d.child(danger_button("forget-default", "Forget default", p).on_activate(act(cx, |this, _, cx| {
                            this.dialog = Some(Dialog::ForgetDefault);
                            cx.notify();
                        })))
                    })
                    .when(self.master_enabled, |d| {
                        d.child(secondary_button("forgot-master", "Forgot master password?", None, p).on_activate(act(cx, |this, _, cx| {
                            this.dialog = Some(Dialog::ForgetAll);
                            cx.notify();
                        })))
                    })
                    .children(message("creds-msg", self.msgs.get("creds"), p)),
            );

        let file = section("AWS credentials file", Some("Open the credentials file in your editor"), p).child(div().flex().child(
            secondary_button("open-credentials-file", "Open credentials file", Some("file-text"), p)
                .on_activate(act(cx, |this, _, cx| this.call("settings:openCredentialsFile", vec![], cx, |_, _, _| {}))),
        ));

        let map = st.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
        let mut rows = div().flex().flex_col().gap(px(8.));
        for (i, (id, input)) in self.names.iter().enumerate() {
            if !map.contains_key(id) {
                continue;
            }
            let remove_id = id.clone();
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id(SharedString::from(format!("account-id-{i}")))
                            .role(Role::Label)
                            // A label's accessible name comes from its value.
                            .aria_label(SharedString::from(format!("Account ID {id}")))
                            .aria_value(SharedString::from(format!("Account ID {id}")))
                            .w(px(144.))
                            .h(px(size::CONTROL))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.border)
                            .font_family(FONT_MONO)
                            .text_size(px(size::TEXT_XS))
                            .text_color(p.text_muted)
                            .child(id.clone()),
                    )
                    .child(div().text_color(p.text_faint).child("→"))
                    .child(div().w(px(280.)).child(Input::new(input).aria_label(SharedString::from(format!("Display name for {id}")))))
                    .child(
                        icon_button(SharedString::from(format!("remove-account-{i}")), "x", SharedString::from(format!("Remove {id}")), p)
                            .on_activate(act(cx, move |this, window, cx| this.remove_account_name(remove_id.clone(), window, cx))),
                    ),
            );
        }
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pt(px(4.))
                .child(div().w(px(176.)).child(Input::new(&self.new_account_id).aria_label("Account ID")))
                .child(div().w(px(280.)).child(Input::new(&self.new_account_name).aria_label("Display name")))
                .child(inline_button("add-account-name", "Add", Some("plus"), p).on_activate(act(cx, |this, window, cx| this.add_account_name(window, cx)))),
        );
        let names = section("Account display names", Some("Map AWS account IDs to friendly names in the role picker when adding or editing a profile"), p)
            .child(div().max_w(px(640.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(
                "The app only receives the SAML form, not the IdP role-picker page, so add mappings here to get friendly labels. Restore defaults resets them from the list stored in settings.json (accountDisplayNamesDefault).",
            ))
            .child(rows)
            .child(hint("Enter Account ID and Display name, then click Add or press Enter.", p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(secondary_button("restore-default-names", "Restore defaults", None, p).on_activate(act(cx, |this, window, cx| this.restore_default_names(window, cx))))
                    .children(message("names-msg", self.msgs.get("names"), p)),
            );

        div().flex().flex_col().gap(px(16.)).child(defaults).child(file).child(names)
    }

    fn render_terminal(&mut self, st: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let key_saved = api_key_saved(st);
        let typed_key = !self.api_key.read(cx).value().is_empty();
        let url_set = !s(st, "openWebUiApiUrl").trim().is_empty();
        if !self.models_requested && url_set && key_saved {
            self.fetch_models(cx);
        }
        let model_required = partially_configured(st);
        let can_refresh = !self.models_loading && url_set && key_saved;

        let terminal = section(
            "Embedded terminal",
            Some(if cfg!(windows) {
                "Defaults for the Terminal screen. Bash uses Git for Windows (git-scm); point to Git's bin\\bash.exe."
            } else {
                "Defaults for the Terminal screen. Bash uses /bin/bash unless you point to another build (e.g. Homebrew's)."
            }),
            p,
        )
        .child(field(
            "Bash executable path",
            div()
                .max_w(px(520.))
                .flex()
                .gap(px(8.))
                .child(div().flex_1().child(Input::new(&self.bash_path).aria_label("Bash executable path")))
                .child(inline_button("browse-bash", "Browse…", None, p).on_activate(act(cx, |this, window, cx| this.browse_bash(window, cx)))),
            Some(if cfg!(windows) { "Use bin\\bash.exe so Bash runs in the app." } else { "Optional. Leave empty to use the system /bin/bash." }),
            p,
        ))
        .child(field(
            "Default profile",
            div().max_w(px(420.)).child(self.terminal_profile.clone()),
            Some("Pre-selected in the Terminal screen so inserted commands get --profile straight away."),
            p,
        ));

        let ai = section("Open WebUI integration", Some("Connects the AI assistant on the Terminal screen to your Open WebUI instance"), p)
            .child(field("API URL", div().max_w(px(520.)).child(Input::new(&self.api_url).aria_label("API URL")), None, p))
            .child(
                field(
                    "API key",
                    div().max_w(px(520.)).child(Input::new(&self.api_key).aria_label("API key").mask_toggle()),
                    Some(if key_saved && !typed_key { "Saved. Type a new key to replace it." } else { "Leaving the field empty keeps the saved key." }),
                    p,
                )
                .when(key_saved && !typed_key, |d| {
                    d.child(div().flex().child(
                        div()
                            .id("remove-api-key")
                            .role(Role::Button)
                            .aria_label("Remove saved key")
                            .focusable()
                            .text_size(px(size::TEXT_SM))
                            .text_color(p.text_muted)
                            .underline()
                            .cursor_pointer()
                            .hover(|d| d.text_color(p.text))
                            .on_activate(act(cx, |this, _, cx| {
                                let mut patch = Map::new();
                                patch.insert("openWebUiApiKey".into(), json!(""));
                                patch.insert("openWebUiApiKeySet".into(), json!(false));
                                this.save_patch(patch, cx);
                            }))
                            .child("Remove saved key"),
                    ))
                }),
            )
            .child(
                div()
                    .max_w(px(520.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(label("Model", p))
                            .when(model_required, |d| d.child(div().text_size(px(size::TEXT_SM)).text_color(p.danger).child("*"))),
                    )
                    .when(model_required && s(st, "openWebUiModel").trim().is_empty(), |d| {
                        d.child(div().text_size(px(size::TEXT_SM)).text_color(p.danger).child("Please select a model to use Open WebUI."))
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().child(self.ai_model.clone()))
                            .child(
                                icon_button("refresh-models", "refresh-cw", "Refresh model list", p)
                                    .when(!can_refresh, |d| d.opacity(0.5).cursor_default())
                                    .on_activate(act(cx, move |this, _, cx| {
                                        if can_refresh {
                                            this.fetch_models(cx)
                                        }
                                    }))
                                    .a11y_disabled(!can_refresh),
                            ),
                    )
                    .when(self.models_loading, |d| d.child(hint("Loading models…", p)))
                    .when_some(self.models_error.clone(), |d, e| d.child(div().text_size(px(size::TEXT_SM)).text_color(p.danger).child(e))),
            );

        div().flex().flex_col().gap(px(16.)).child(terminal).child(ai)
    }

    fn render_advanced(&mut self, st: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let backup = section("Backup & restore", None, p)
            .child(dialog_body("Save your settings and profiles to a file, or restore them from one. Restoring replaces the current config after you confirm.", p))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(primary_button("backup-config", "Back up config", Some("download"), p).on_activate(act(cx, |this, _, cx| this.backup(cx))))
                    .child(secondary_button("restore-config", "Restore config", Some("upload"), p).on_activate(act(cx, |this, _, cx| this.restore(cx)))),
            )
            .children(message("backup-msg", self.msgs.get("backup"), p));

        let mut out = div().flex().flex_col().gap(px(16.)).child(backup);
        if self.dev_unlocked {
            let checkbox = |id: &'static str, text: &'static str, checked: bool, key: &'static str, cx: &mut Context<Self>| {
                setting_checkbox(id, text, checked, key, cx)
            };
            let debug = section("Developer options", None, p)
                .child(hint("Auth audit logs IdP sign-in attempts and failures (rolling 5-day retention). Logging is on by default; disable to stop writing new entries.", p))
                .child(checkbox("auth-logging", "Enable auth audit logging", st.get("authLoggingEnabled") != Some(&Value::Bool(false)), "authLoggingEnabled", cx))
                .child(div().flex().child(
                    secondary_button("view-auth-logs", "View auth logs", None, p).on_activate(act(cx, |this, _, cx| this.call("logs:openAuthViewer", vec![], cx, |_, _, _| {}))),
                ))
                .child(div().h(px(1.)).bg(p.border))
                .child(hint("Pre-release updates: when on, \"Check for updates\" will consider GitHub releases tagged as pre-release (e.g. v1.2.4-rc.1). Turn off when you're done.", p))
                .child(checkbox("allow-prerelease", "Allow pre-release updates", st.get("allowPrerelease") == Some(&Value::Bool(true)), "allowPrerelease", cx))
                .child(div().h(px(1.)).bg(p.border))
                .child(hint("Forgets which release notes and feature tours have been shown, so the next launch takes the real first-run-after-update path.", p))
                .child(div().flex().child(secondary_button("reset-whats-new", "Reset “what’s new” state", None, p).on_activate(act(cx, |this, _, cx| {
                    this.call("tour:resetWhatsNew", vec![], cx, |this, _, cx| this.set_msg("whatsnew", false, "Cleared — they will show again on the next launch.", cx))
                }))))
                .children(message("whatsnew-msg", self.msgs.get("whatsnew"), p));
            out = out.child(debug);
        }
        out
    }

    fn render_dialog(&mut self, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let dialog = self.dialog.as_ref()?;
        let actions = |cancel: Stateful<gpui::Div>, confirm: Stateful<gpui::Div>| div().flex().justify_end().gap(px(8.)).child(cancel).child(confirm);
        let card = match dialog {
            Dialog::Restore(v) => {
                let n = v.get("profiles").and_then(Value::as_array).map_or(0, Vec::len);
                dialog_card("restore-dialog", "Restore config", true, p)
                    .child(dialog_title("Restore config"))
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(format!(
                        "Restore from backup? This will replace your current settings and {n} profile(s). Your current data will be overwritten."
                    )))
                    .child(actions(
                        secondary_button("restore-cancel", "Cancel", None, p).on_activate(act(cx, |this, _, cx| this.close_dialog(cx))),
                        primary_button("restore-confirm", "Restore", None, p).on_activate(act(cx, |this, window, cx| this.apply_restore(window, cx))),
                    ))
            }
            Dialog::ForgetDefault => dialog_card("forget-default-dialog", "Remove default credentials?", true, p)
                .child(dialog_title("Remove default credentials?"))
                .child(dialog_body("You will be prompted for username and password for each profile that uses default credentials.", p))
                .child(actions(
                    secondary_button("forget-cancel", "Cancel", None, p).on_activate(act(cx, |this, _, cx| this.close_dialog(cx))),
                    primary_button("forget-confirm", "Forget default", None, p).bg(p.danger_solid).on_activate(act(cx, |this, window, cx| this.forget_default(window, cx))),
                )),
            Dialog::ForgetAll => dialog_card("forget-all-dialog", "Remove all saved credentials?", true, p)
                .child(dialog_title("Remove all saved credentials?"))
                .child(dialog_body("All saved IdP credentials and your master password will be removed.", p))
                .child(dialog_body(
                    "You'll be prompted for IdP login on each refresh until you save default credentials again. Saving them again will require a new master password.",
                    p,
                ))
                .child(actions(
                    secondary_button("forget-all-cancel", "Cancel", None, p).on_activate(act(cx, |this, _, cx| this.close_dialog(cx))),
                    primary_button("forget-all-confirm", "Remove all and reset", None, p).bg(p.danger_solid).on_activate(act(cx, |this, window, cx| this.forget_all(window, cx))),
                )),
            Dialog::Master(d) => self.render_master(d, p, cx),
        };
        let viewport = window.viewport_size();
        Some(
            deferred(
                anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(0.))).child(
                    div()
                        .id("settings-dialog-scrim")
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .bg(p.scrim)
                        .flex()
                        .items_center()
                        .justify_center()
                        // A click outside cancels, as in 1.x.
                        .on_activate(act(cx, |this, _, cx| this.close_dialog(cx)))
                        .child(div().id("settings-dialog-body").on_click(|_, _, cx| cx.stop_propagation()).child(card)),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn render_master(&self, d: &MasterDialog, p: &Palette, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let header = div()
            .flex()
            .items_start()
            .gap(px(14.))
            .child(
                div()
                    .size(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.sunken)
                    .child(icon("lock", 18., p.text_muted)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(dialog_title("Master password"))
                    .when(!d.acknowledged, |el| {
                        el.child(div().text_size(px(size::TEXT_SM)).text_color(p.text).child(
                            "We require a master password to encrypt your saved credentials. You'll enter it once each time you start the Profile Manager.",
                        ))
                    }),
            );
        let card = dialog_card("master-password-dialog", "Master password", false, p).child(header);
        if !d.acknowledged {
            let progress = d.progress;
            let held = d.hold_started.is_some();
            let is_activation = |key: &str| key == "space" || key == "enter";
            let hold = div()
                .id("hold-to-confirm")
                .role(Role::Button)
                .aria_label("Hold to confirm")
                .aria_description("Press and hold for one second to confirm")
                .track_focus(&d.hold_focus)
                .tab_stop(true)
                .relative()
                .overflow_hidden()
                .h(px(48.))
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size::RADIUS))
                .bg(p.primary)
                .text_color(p.on_primary)
                .text_size(px(size::TEXT))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(!held, |el| el.hover(|el| el.opacity(0.92)))
                .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                    if is_activation(&ev.keystroke.key) && !ev.is_held {
                        this.hold_start(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .on_key_up(cx.listener(move |this, ev: &KeyUpEvent, _, cx| {
                    if is_activation(&ev.keystroke.key) {
                        this.hold_end(cx);
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.hold_start(window, cx)))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.hold_end(cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.hold_end(cx)))
                .child(div().absolute().left_0().top_0().bottom_0().w(relative(progress)).bg(gpui::hsla(0., 0., 1., 0.2)))
                .child(div().relative().child(if progress >= 1. { "Done" } else { "Hold to confirm" }));
            return card.child(hold);
        }

        let password = d.password.read(cx).value().to_string();
        let confirm = d.confirm.read(cx).value().to_string();
        let n = apm_core::master_password::length(&password);
        let min = apm_core::master_password::MIN_LENGTH;
        let long_enough = n >= min;
        let length_text = if long_enough {
            format!("{min}+ characters")
        } else if password.is_empty() {
            format!("At least {min} characters")
        } else {
            format!("At least {min} characters ({n}/{min})")
        };
        let status_line = |id: &'static str, text: String, ok: Option<bool>| {
            let color = match ok {
                Some(true) => p.success,
                Some(false) => p.danger,
                None => p.text_muted,
            };
            div()
                .id(id)
                .role(Role::Label)
                // A label's accessible name comes from its value.
                .aria_label(SharedString::from(text.clone()))
                .aria_value(SharedString::from(text.clone()))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(size::TEXT_XS))
                .text_color(color)
                .when(ok == Some(true), |el| el.child(icon("circle-check", 14., color)))
                .when(ok == Some(false), |el| el.child(icon("circle-x", 14., color)))
                .child(text)
        };
        let form = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(16.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.sunken)
            .child(hint(apm_core::master_password::REQUIREMENTS, p))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("Password", p))
                    .child(Input::new(&d.password).aria_label("Password").mask_toggle())
                    .child(status_line("master-length", length_text, long_enough.then_some(true))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("Confirm", p))
                    .child(Input::new(&d.confirm).aria_label("Confirm").mask_toggle())
                    .when(!confirm.is_empty(), |el| {
                        let same = password == confirm;
                        el.child(status_line("master-match", if same { "Passwords match".into() } else { "Passwords don't match".into() }, Some(same)))
                    }),
            );
        let error = d.error.clone().map(|e| {
            div().id("master-error").role(Role::Alert).aria_label(SharedString::from(e.clone())).text_size(px(size::TEXT_SM)).text_color(p.danger).child(e)
        });
        let submitting = d.submitting;
        card.child(form).children(error).child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(secondary_button("master-cancel", "Cancel", None, p).on_activate(act(cx, |this, _, cx| this.close_dialog(cx))).a11y_disabled(submitting))
                .child(
                    primary_button("master-submit", if submitting { "Please wait..." } else { "Create & save" }, None, p)
                        .aria_label("Create & save")
                        .on_activate(act(cx, |this, window, cx| this.create_master_and_save(window, cx)))
                        .a11y_disabled(submitting),
                ),
        )
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.model.read(cx).palette;
        let st = self.settings(cx);
        let tab = self.tab;

        let mut tabs = div().id("settings-tabs").role(Role::TabList).aria_label("Settings sections").flex().gap(px(4.)).border_b_1().border_color(p.border);
        for (t, id, text) in TABS {
            let on = t == tab;
            tabs = tabs.child(
                div()
                    .id(id)
                    .role(Role::Tab)
                    .aria_label(text)
                    .aria_selected(on)
                    .focusable()
                    .tab_stop(true)
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_b_2()
                    .border_color(if on { p.accent } else { gpui::transparent_black().into() })
                    .text_size(px(size::TEXT_SM))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if on { p.text } else { p.text_muted })
                    .cursor_pointer()
                    .hover(|d| d.text_color(p.text))
                    .on_activate(act(cx, move |this, _, cx| {
                        this.tab = t;
                        cx.notify();
                    }))
                    .child(text),
            );
        }

        let body = if !self.loaded {
            div()
        } else {
            match tab {
                Tab::General => self.render_general(&st, &p, cx),
                Tab::Appearance => self.render_appearance(&st, &p, cx),
                Tab::Credentials => self.render_credentials(&st, &p, cx),
                Tab::Terminal => self.render_terminal(&st, &p, cx),
                Tab::Advanced => self.render_advanced(&st, &p, cx),
            }
        };
        let dialog = self.render_dialog(&p, window, cx);

        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .bg(p.bg)
            .child(
                div()
                    .max_w(px(820.))
                    .px(px(24.))
                    .py(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(div().id("settings-heading").role(Role::Heading).aria_label("Settings").text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                    .child(tabs)
                    .child(body),
            )
            .children(dialog)
    }
}
