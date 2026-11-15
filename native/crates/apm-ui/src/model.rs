//! App state shared by the views: what the core last reported, plus view-only state (current page,
//! selection, search). Views read it and call `Bridge` channels; core events update it.

use std::collections::HashSet;
use std::time::Duration;

use gpui::{Context, Task, WindowAppearance};
use serde_json::Value;

use crate::bridge::{Bridge, CoreEvent};
use crate::dialogs::{CredPurpose, DialogKind};
pub use crate::onboarding::{ActiveTour, Changelog};
use crate::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Profiles,
    Terminal,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Expiring,
    NeedsSignIn,
}

/// What the profile form opens with.
#[derive(Debug, Clone)]
pub struct FormRequest {
    pub profile: Value,
    pub is_new: bool,
    /// `settings.accountDisplayNames`.
    pub names: serde_json::Map<String, Value>,
    /// `settings.defaultSsoStartUrl` / `defaultSsoRegion`.
    pub sso_defaults: (String, String),
}

/// Master password gate (1.x `MasterPasswordState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Loading,
    NeedsCreate,
    NeedsUnlock,
    Unlocked,
}

/// A short-lived message at the bottom of the window (1.x `notify` toasts and errors).
#[derive(Debug, Clone)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub error: bool,
    /// Tells toasts apart, so each one animates in (and a newer one is not cleared by an older
    /// one's timer).
    pub seq: u64,
    /// In its last moments: fading out.
    pub leaving: bool,
}

/// A copy of what a render needs, so views do not hold a borrow of the app while they build
/// click handlers (which need `&mut` access).
#[derive(Clone)]
pub struct Snap {
    pub palette: Palette,
    pub page: Page,
    pub profiles: Vec<Value>,
    pub dashboard: Vec<Value>,
    pub folders: Vec<Value>,
    pub collapsed_folders: HashSet<String>,
    pub selected: Option<String>,
    pub search: String,
    pub filter: Filter,
    pub refreshing: HashSet<String>,
    pub toast: Option<Toast>,
    pub loaded: bool,
}

impl Snap {
    pub fn summary(&self, id: &str) -> Option<&Value> {
        self.dashboard.iter().find(|d| d["id"] == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Value> {
        self.profiles.iter().find(|p| p["id"] == id)
    }
}

pub struct AppModel {
    pub bridge: Bridge,
    pub palette: Palette,
    pub os_appearance: WindowAppearance,
    pub page: Page,
    pub profiles: Vec<Value>,
    /// `dashboard:getState`, in profile order.
    pub dashboard: Vec<Value>,
    pub folders: Vec<Value>,
    pub collapsed_folders: HashSet<String>,
    pub settings: Value,
    pub selected: Option<String>,
    pub search: String,
    pub filter: Filter,
    pub refreshing: HashSet<String>,
    pub toast: Option<Toast>,
    pub loaded: bool,
    pub lock: LockState,
    /// The open modal; `dialog_seq` changes each time one opens so the shell builds a fresh view.
    pub dialog: Option<DialogKind>,
    pub dialog_seq: u64,
    /// Handed from `handle_refresh_result` to the open dialog (wrong password and similar).
    pub dialog_error: Option<String>,
    /// The profile form, when open; `form_seq` changes each time it opens.
    pub form: Option<FormRequest>,
    pub form_seq: u64,
    /// Roles fetched by the "Sign in to load roles" dialog, for the form: (IdP URL, roles).
    pub loaded_roles: Option<(String, Vec<Value>)>,
    /// The first-run setup wizard is open; `wizard_seq` changes each time it opens.
    pub wizard_open: bool,
    pub wizard_seq: u64,
    /// First run, or "Add accounts…" with only the import steps.
    pub wizard_mode: crate::wizard::WizardMode,
    /// Release notes waiting to be shown ("What’s new").
    pub changelog: Option<Changelog>,
    /// The tour due this launch, not yet shown (index into `onboarding::TOURS`). Non-null is what
    /// makes the release notes offer "Start tour".
    pub pending_tour: Option<usize>,
    pub tour: Option<ActiveTour>,
    /// "Auto-refresh paused" alert after repeated sign-in failures.
    pub paused_alert: bool,
    /// `scheduler:getPaused`: (paused, pausedDueToFailures).
    pub refresh_paused: (bool, bool),
    /// A Settings tab to switch to (the tour points at Appearance).
    pub settings_tab_request: Option<crate::settings::Tab>,
    /// The navigation sidebar shows icons only (1.x `ui:getSidebarCollapsed`; 1.x default: expanded).
    pub sidebar_collapsed: bool,
    /// The selected app icon (Settings → Appearance), for the title bar (1.x `app:iconChanged`).
    pub app_icon: Option<std::path::PathBuf>,
    /// The background video is on screen behind the app ("glass"): set by the shell each frame,
    /// read through [`AppModel::surface_palette`] by the views drawn over it.
    pub glass: bool,
    /// The background video is on screen behind the sign-in (master password) screen.
    pub glass_lock: bool,
    /// Mean luminance of the video on screen (0–1), for translucent surfaces' opacity.
    pub glass_luminance: f32,
    /// The blur slider's value while it is being dragged (saved on release), for live feedback.
    pub blur_preview: Option<f32>,
    pub(crate) onboarding_started: bool,
    pub(crate) changelog_checked: bool,
    theme_synced: bool,
    /// One settings save at a time (`save_settings_patch`).
    save_lock: std::sync::Arc<futures::lock::Mutex<()>>,
    _tick: Option<Task<()>>,
}

impl AppModel {
    pub fn new(bridge: Bridge, os_appearance: WindowAppearance, cx: &mut Context<Self>) -> Self {
        let mut this = AppModel {
            bridge,
            palette: Palette::from_settings(None, None, os_appearance),
            os_appearance,
            page: Page::Profiles,
            profiles: Vec::new(),
            dashboard: Vec::new(),
            folders: Vec::new(),
            collapsed_folders: HashSet::new(),
            settings: Value::Null,
            selected: None,
            search: String::new(),
            filter: Filter::All,
            refreshing: HashSet::new(),
            toast: None,
            loaded: false,
            lock: LockState::Loading,
            dialog: None,
            dialog_seq: 0,
            dialog_error: None,
            form: None,
            form_seq: 0,
            loaded_roles: None,
            wizard_open: false,
            wizard_seq: 0,
            wizard_mode: crate::wizard::WizardMode::FirstRun,
            changelog: None,
            pending_tour: None,
            tour: None,
            paused_alert: false,
            refresh_paused: (false, false),
            settings_tab_request: None,
            sidebar_collapsed: false,
            app_icon: None,
            glass: false,
            glass_lock: false,
            glass_luminance: 0.0,
            blur_preview: None,
            onboarding_started: false,
            changelog_checked: false,
            theme_synced: false,
            save_lock: Default::default(),
            _tick: None,
        };
        this.reload(cx);
        this.check_lock(cx);
        this.load_sidebar(cx);
        // Countdowns move: re-read the dashboard every 30 s (cheap: local files only).
        this._tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(30)).await;
            if this.update(cx, |m, cx| m.reload_dashboard(cx)).is_err() {
                break;
            }
        }));
        this
    }

    /// The palette for surfaces in the main window's page area: translucent while the background
    /// video plays behind the app. Dialogs, menus and other windows use `palette` (always solid).
    pub fn surface_palette(&self) -> Palette {
        if self.glass {
            self.palette.glass()
        } else {
            self.palette
        }
    }

    /// Background settings (1.x `resolveConfig`): the stored video path (may no longer exist), the
    /// scope ("off" / "auth" / "app"; default "auth" as in 1.x) and the blur (0–40 px, default 12).
    pub fn background(&self) -> (Option<std::path::PathBuf>, String, f32) {
        let st = &self.settings;
        let path = st
            .get("backgroundVideoPath")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from);
        let scope = st.get("backgroundScope").and_then(Value::as_str).unwrap_or("auth").to_string();
        let blur = self
            .blur_preview
            .or_else(|| st.get("backgroundBlur").and_then(Value::as_f64).map(|v| v as f32))
            .unwrap_or(crate::video::DEFAULT_BLUR)
            .round()
            .clamp(crate::video::MIN_BLUR, crate::video::MAX_BLUR);
        (path, scope, blur)
    }

    pub fn snapshot(&self) -> Snap {
        Snap {
            palette: self.surface_palette(),
            page: self.page,
            profiles: self.profiles.clone(),
            dashboard: self.dashboard.clone(),
            folders: self.folders.clone(),
            collapsed_folders: self.collapsed_folders.clone(),
            selected: self.selected.clone(),
            search: self.search.clone(),
            filter: self.filter,
            refreshing: self.refreshing.clone(),
            toast: self.toast.clone(),
            loaded: self.loaded,
        }
    }

    /// Re-read everything the views show.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let profiles = b.call("profiles:getAll", vec![]).await;
            let dashboard = b.call("dashboard:getState", vec![]).await;
            let folders = b.call("folders:getAll", vec![]).await;
            let settings = b.call("settings:get", vec![]).await;
            let collapsed = b.call("ui:getCollapsedFolders", vec![]).await;
            let paused = b.call("scheduler:getPaused", vec![]).await;
            let _ = this.update(cx, |m, cx| {
                if let Ok(v) = paused {
                    m.refresh_paused = pause_state(&v);
                }
                m.profiles = list(profiles);
                m.dashboard = list(dashboard);
                m.folders = list(folders);
                m.collapsed_folders = list(collapsed).iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                if let Ok(s) = settings {
                    m.settings = s;
                }
                m.apply_palette(cx);
                m.app_icon = Some(app_icon_png(&m.bridge));
                if m.selected.as_ref().is_some_and(|id| !m.profiles.iter().any(|p| p["id"] == id.as_str())) {
                    m.selected = None;
                }
                m.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn reload_dashboard(&mut self, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let dashboard = b.call("dashboard:getState", vec![]).await;
            let _ = this.update(cx, |m, cx| {
                m.dashboard = list(dashboard);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn apply_palette(&mut self, cx: &mut gpui::App) {
        let s = |k: &str| self.settings.get(k).and_then(Value::as_str).map(str::to_string);
        let next = Palette::from_settings(s("theme").as_deref(), s("appStyle").as_deref(), self.os_appearance);
        let changed = (next.mode, next.style) != (self.palette.mode, self.palette.style) || !self.theme_synced;
        self.palette = next;
        cx.set_global(crate::picker::PickerPalette(next));
        if changed {
            self.theme_synced = true;
            crate::theme::sync_component_theme(&self.palette, cx);
        }
    }

    pub fn set_os_appearance(&mut self, a: WindowAppearance, cx: &mut Context<Self>) {
        if self.os_appearance != a {
            self.os_appearance = a;
            self.apply_palette(cx);
            cx.notify();
        }
    }

    pub fn summary(&self, id: &str) -> Option<&Value> {
        self.dashboard.iter().find(|d| d["id"] == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Value> {
        self.profiles.iter().find(|p| p["id"] == id)
    }

    pub fn show_toast(&mut self, title: impl Into<String>, body: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.toast = Some(Toast { title: title.into(), body: body.into(), error, seq, leaving: false });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let fade = crate::motion::TOAST;
            cx.background_executor().timer(Duration::from_secs(5) - fade).await;
            let _ = this.update(cx, |m, cx| {
                if let Some(t) = m.toast.as_mut().filter(|t| t.seq == seq) {
                    t.leaving = true;
                    cx.notify();
                }
            });
            cx.background_executor().timer(fade).await;
            let _ = this.update(cx, |m, cx| {
                if m.toast.as_ref().is_some_and(|t| t.seq == seq) {
                    m.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Call a channel, then reload; failures become an error toast.
    pub fn run(&mut self, channel: &'static str, args: Vec<Value>, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = b.call(channel, args).await;
            let _ = this.update(cx, |m, cx| {
                if let Err(e) = result {
                    m.show_toast("Something went wrong", e, true, cx);
                }
                m.reload(cx);
            });
        })
        .detach();
    }

    pub fn refresh(&mut self, id: String, cx: &mut Context<Self>) {
        self.refreshing.insert(id.clone());
        cx.notify();
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = b.call("auth:refresh", vec![Value::String(id.clone())]).await;
            let _ = this.update(cx, |m, cx| match result {
                Ok(v) => {
                    if let Some(e) = m.handle_refresh_result(&id, v, cx) {
                        m.show_toast("Refresh failed", e, true, cx);
                    }
                }
                Err(e) => {
                    m.refreshing.remove(&id);
                    m.show_toast("Refresh failed", e, true, cx);
                    m.reload(cx);
                }
            });
        })
        .detach();
    }

    /// The 1.x `handleRefresh` branches for an `auth:refresh` / `auth:submitCredentials` result:
    /// SSO sign-in, IdP password, role choice, success. Returns the error text when it failed
    /// (the caller shows it where the user is looking).
    pub fn handle_refresh_result(&mut self, id: &str, v: Value, cx: &mut Context<Self>) -> Option<String> {
        if v.get("required") == Some(&Value::Bool(true)) {
            // Keep a dialog already open for this profile (wrong password): the caller shows the error.
            let open_for_same = matches!(&self.dialog, Some(DialogKind::Credentials { purpose: CredPurpose::Profile { id: d }, .. }) if d == id);
            if !open_for_same {
                let prefill = v.get("prefillUsername").and_then(Value::as_str).unwrap_or("").to_string();
                self.open_credentials(id, prefill, cx);
            }
            return v.get("error").and_then(Value::as_str).map(str::to_string);
        }
        if let Some(Value::Array(roles)) = v.get("roles") {
            let name = self.profile_name(id);
            self.open_dialog(DialogKind::Roles { profile_id: id.to_string(), profile_name: name, roles: roles.clone() }, cx);
            return None;
        }
        self.refreshing.remove(id);
        if v.get("ssoLoginRequired") == Some(&Value::Bool(true)) {
            // Not an error: the SSO session needs a human. Offer the sign-in rather than shouting.
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let (start_url, region) = (s("startUrl"), s("region"));
            if !start_url.is_empty() && !region.is_empty() {
                self.open_dialog(DialogKind::SsoNotice { start_url, region }, cx);
            } else {
                self.close_dialog(cx);
            }
            self.reload(cx);
            return None;
        }
        if v.get("success") == Some(&Value::Bool(false)) {
            let e = v.get("error").and_then(Value::as_str).unwrap_or("Refresh failed").to_string();
            self.reload(cx);
            return Some(e);
        }
        self.close_dialog(cx);
        self.reload(cx);
        None
    }

    fn profile_name(&self, id: &str) -> String {
        self.profile(id).and_then(|p| p["name"].as_str()).unwrap_or(id).to_string()
    }

    pub fn open_credentials(&mut self, id: &str, prefill: String, cx: &mut Context<Self>) {
        let name = self.profile_name(id);
        self.open_dialog(
            DialogKind::Credentials {
                purpose: CredPurpose::Profile { id: id.to_string() },
                title: format!("Sign in for {name}"),
                description: "Enter your IdP username and password. To skip this prompt next time, set default credentials in Settings.".into(),
                prefill,
            },
            cx,
        );
    }

    /// Move one profile to a folder (`None`: no folder), keeping the order (1.x applyProfileLayout).
    pub fn move_to_folder(&mut self, id: &str, folder: Option<String>, cx: &mut Context<Self>) {
        let order: Vec<Value> = self.profiles.iter().filter_map(|p| p.get("id").cloned()).collect();
        let mut map = serde_json::Map::new();
        map.insert(id.to_string(), folder.map(Value::String).unwrap_or(Value::Null));
        self.run("profiles:applyLayout", vec![Value::Array(order), Value::Object(map)], cx);
    }

    /// Open the profile form: `None` adds a profile.
    pub fn edit_profile(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let settings = b.call("settings:get", vec![]).await.unwrap_or(Value::Null);
            let st = |k: &str| settings.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let names = settings.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
            let sso_defaults = (st("defaultSsoStartUrl"), st("defaultSsoRegion"));
            let (profile, is_new) = match id {
                Some(id) => match b.call("profiles:getById", vec![Value::String(id)]).await {
                    Ok(p @ Value::Object(_)) => (p, false),
                    _ => return,
                },
                None => {
                    // 1.x startAdd: defaults from Settings; default credentials on when saved.
                    let has_default = !matches!(b.call("credentials:getDefaultDisplay", vec![]).await, Ok(Value::Null) | Err(_));
                    let hours = settings.get("defaultSessionDurationHours").and_then(Value::as_f64).unwrap_or(1.0);
                    let p = serde_json::json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "name": "",
                        "idpEntryUrl": st("defaultIdpEntryUrl"),
                        "label": "",
                        "autoRefresh": false,
                        "refreshIntervalMinutes": ((hours * 60.0).floor() as i64).max(60),
                        "useDefaultCredentials": has_default,
                        "credentialProfileName": "",
                        "authType": "saml",
                        "ssoStartUrl": sso_defaults.0,
                        "ssoRegion": sso_defaults.1,
                    });
                    (p, true)
                }
            };
            let _ = this.update(cx, |m, cx| {
                m.form = Some(FormRequest { profile, is_new, names, sso_defaults });
                m.form_seq += 1;
                m.loaded_roles = None;
                m.page = Page::Profiles;
                cx.notify();
            });
        })
        .detach();
    }

    /// "Add accounts…": the wizard with only the import steps (1.x `wizard:openImport`).
    pub fn open_import_wizard(&mut self, cx: &mut Context<Self>) {
        self.form = None;
        self.wizard_mode = crate::wizard::WizardMode::Import;
        self.wizard_open = true;
        self.wizard_seq += 1;
        cx.notify();
    }

    pub fn close_form(&mut self, cx: &mut Context<Self>) {
        if self.form.take().is_some() {
            cx.notify();
        }
    }

    pub fn open_dialog(&mut self, kind: DialogKind, cx: &mut Context<Self>) {
        self.dialog = Some(kind);
        self.dialog_seq += 1;
        self.dialog_error = None;
        cx.notify();
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        if std::env::var("APM_TRACE_DIALOG").is_ok() {
            eprintln!("TRACE close_dialog
{}", std::backtrace::Backtrace::force_capture());
        }
        if self.dialog.take().is_some() {
            cx.notify();
        }
    }

    /// The user dismissed a dialog (Cancel, Escape, click outside).
    pub fn dialog_closed(&mut self, kind: &DialogKind, cx: &mut Context<Self>) {
        match kind {
            DialogKind::Credentials { purpose: CredPurpose::Profile { id }, .. } | DialogKind::Roles { profile_id: id, .. } => {
                self.refreshing.remove(id);
            }
            DialogKind::Credentials { purpose: CredPurpose::RefreshAll { credential_ids, .. }, .. } => {
                for id in credential_ids {
                    self.refreshing.remove(id);
                }
            }
            DialogKind::Confirm { .. } | DialogKind::SsoNotice { .. } | DialogKind::Credentials { purpose: CredPurpose::LoadRoles { .. }, .. } => {}
        }
        self.close_dialog(cx);
    }

    fn check_lock(&mut self, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let status = b.call("credentials:getMasterPasswordStatus", vec![]).await.unwrap_or(Value::Null);
            let _ = this.update(cx, |m, cx| {
                m.lock = if status.get("needsCreateMasterPassword") == Some(&Value::Bool(true)) {
                    LockState::NeedsCreate
                } else if status.get("needsUnlock") == Some(&Value::Bool(true)) {
                    LockState::NeedsUnlock
                } else {
                    LockState::Unlocked
                };
                if m.lock == LockState::Unlocked {
                    m.start_onboarding(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn unlocked(&mut self, cx: &mut Context<Self>) {
        self.lock = LockState::Unlocked;
        self.reload(cx);
        self.start_onboarding(cx);
        cx.notify();
    }

    pub fn open_console(&mut self, id: String, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = b.call("console:open", vec![Value::String(id)]).await;
            let _ = this.update(cx, |m, cx| match result {
                Ok(v) if v.get("success") == Some(&Value::Bool(false)) => {
                    let e = v.get("error").and_then(Value::as_str).unwrap_or("Could not open the console").to_string();
                    m.show_toast("Could not open the console", e, true, cx);
                }
                Err(e) => m.show_toast("Could not open the console", e, true, cx),
                _ => {}
            });
        })
        .detach();
    }

    fn load_sidebar(&mut self, cx: &mut Context<Self>) {
        let b = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Value::Bool(collapsed)) = b.call("ui:getSidebarCollapsed", vec![]).await {
                let _ = this.update(cx, |m, cx| {
                    m.sidebar_collapsed = collapsed;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Collapse or expand the sidebar and remember it (1.x `ui:setSidebarCollapsed`).
    pub fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.sidebar_collapsed = collapsed;
        cx.notify();
        let b = self.bridge.clone();
        cx.spawn(async move |_, _| {
            let _ = b.call("ui:setSidebarCollapsed", vec![Value::Bool(collapsed)]).await;
        })
        .detach();
    }

    /// The profile list layout setting: cards (1.x grid) or the 2.0 compact rows (default).
    pub fn cards_layout(&self) -> bool {
        self.settings.get("profileListLayout").and_then(Value::as_str) == Some("cards")
    }

    pub fn toggle_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed_folders.remove(id) {
            self.collapsed_folders.insert(id.to_string());
        }
        let ids: Vec<Value> = self.collapsed_folders.iter().cloned().map(Value::String).collect();
        cx.notify();
        let b = self.bridge.clone();
        cx.spawn(async move |_, _| {
            let _ = b.call("ui:setCollapsedFolders", vec![Value::Array(ids)]).await;
        })
        .detach();
    }

    pub(crate) fn save_lock(&self) -> std::sync::Arc<futures::lock::Mutex<()>> {
        self.save_lock.clone()
    }

    /// Save one settings key (theme, style...) and apply it.
    pub fn set_setting(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        let mut patch = serde_json::Map::new();
        patch.insert(key.to_string(), value);
        self.save_settings_patch(patch, cx);
    }

    /// Save some settings keys. `settings:save` replaces the whole object, and the core changes
    /// settings on its own (master password, background video, org import), so each save re-reads
    /// the stored settings and merges the patch into them, one save at a time. The local copy
    /// changes at once so the UI (theme, pressed states) follows the click without a round trip.
    pub fn save_settings_patch(&mut self, patch: serde_json::Map<String, Value>, cx: &mut Context<Self>) {
        if !self.settings.is_object() {
            self.settings = Value::Object(serde_json::Map::new());
        }
        if let Some(obj) = self.settings.as_object_mut() {
            for (k, v) in &patch {
                obj.insert(k.clone(), v.clone());
            }
        }
        self.apply_palette(cx);
        cx.notify();
        let b = self.bridge.clone();
        let lock = self.save_lock.clone();
        cx.spawn(async move |this, cx| {
            let saved = {
                let _guard = lock.lock().await;
                let mut fresh = match b.call("settings:get", vec![]).await {
                    Ok(Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
                for (k, v) in patch {
                    fresh.insert(k, v);
                }
                b.call("settings:save", vec![Value::Object(fresh)]).await
            };
            let settings = b.call("settings:get", vec![]).await;
            let _ = this.update(cx, |m, cx| {
                if let Err(e) = saved {
                    m.show_toast("Could not save settings", e, true, cx);
                }
                if let Ok(s @ Value::Object(_)) = settings {
                    m.settings = s;
                    m.apply_palette(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// An event from the core.
    pub fn on_core_event(&mut self, event: CoreEvent, cx: &mut Context<Self>) {
        let first = event.payload.first().and_then(Value::as_str).map(str::to_string);
        match event.channel.as_str() {
            "auth:refreshStarted" => {
                if let Some(id) = first {
                    self.refreshing.insert(id);
                }
                cx.notify();
            }
            "scheduler:pausedChanged" => {
                if let Some(v) = event.payload.first() {
                    self.refresh_paused = pause_state(v);
                }
                self.reload(cx);
            }
            "auth:autoRefreshPausedForFailures" => {
                self.paused_alert = true;
                cx.notify();
            }
            "auth:credentialsRefreshed" | "auth:credentialsExpired" | "auth:networkUnavailable" => {
                if let Some(id) = &first {
                    self.refreshing.remove(id);
                }
                self.reload(cx);
            }
            "app:iconChanged" => {
                self.app_icon = Some(app_icon_png(&self.bridge));
                cx.notify();
            }
            "notify" => {
                let body = event.payload.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                self.show_toast(first.unwrap_or_default(), body, false, cx);
            }
            "auth:credentialsRequired" => {
                // A background refresh (scheduler, tray) needs the IdP password: ask, unless a dialog is up.
                if let Some(id) = first {
                    if self.dialog.is_none() {
                        let prefill = event.payload.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                        self.refreshing.insert(id.clone());
                        self.open_credentials(&id, prefill, cx);
                    }
                }
            }
            "auth:refreshAllRequired" => {
                let ids = |i: usize| -> Vec<String> {
                    event.payload.get(i).and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
                };
                let (credential_ids, default_ids) = (ids(0), ids(1));
                for id in &credential_ids {
                    self.refreshing.insert(id.clone());
                }
                self.open_dialog(
                    DialogKind::Credentials {
                        purpose: CredPurpose::RefreshAll { credential_ids, default_ids },
                        title: "Sign in to refresh all profiles".into(),
                        description: "These credentials will be used for all profiles that don't use default credentials. Profiles using default credentials will be refreshed separately.".into(),
                        prefill: String::new(),
                    },
                    cx,
                );
            }
            "auth:ssoLoginRequired" => {
                if let Some(id) = &first {
                    self.refreshing.remove(id);
                }
                // The scheduler found an org whose SSO session needs a human. Offer a sign-in; never
                // auto-open one.
                let s = |i: usize| event.payload.get(i).and_then(Value::as_str).unwrap_or("").to_string();
                let (start_url, region) = (s(1), s(2));
                if !start_url.is_empty() {
                    self.open_dialog(DialogKind::SsoNotice { start_url, region }, cx);
                }
            }
            "credentials:masterPasswordReset" => self.unlocked(cx),
            _ => {}
        }
    }
}

fn pause_state(v: &Value) -> (bool, bool) {
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    (b("paused"), b("pausedDueToFailures"))
}

fn list(v: Result<Value, String>) -> Vec<Value> {
    match v {
        Ok(Value::Array(a)) => a,
        _ => Vec::new(),
    }
}

/// PNG of the selected app icon (a local file read; no network).
fn app_icon_png(bridge: &Bridge) -> std::path::PathBuf {
    let core = bridge.core();
    core.app_icons().png_path(&core.current_app_icon_key())
}
