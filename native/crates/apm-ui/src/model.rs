//! App state shared by the views: what the core last reported, plus view-only state (current page,
//! selection, search). Views read it and call `Bridge` channels; core events update it.

use std::collections::HashSet;
use std::time::Duration;

use gpui::{Context, Task, WindowAppearance};
use serde_json::Value;

use crate::bridge::{Bridge, CoreEvent};
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

/// A short-lived message at the bottom of the window (1.x `notify` toasts and errors).
#[derive(Debug, Clone)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub error: bool,
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
    theme_synced: bool,
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
            theme_synced: false,
            _tick: None,
        };
        this.reload(cx);
        // Countdowns move: re-read the dashboard every 30 s (cheap: local files only).
        this._tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(30)).await;
            if this.update(cx, |m, cx| m.reload_dashboard(cx)).is_err() {
                break;
            }
        }));
        this
    }

    pub fn snapshot(&self) -> Snap {
        Snap {
            palette: self.palette,
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
            let _ = this.update(cx, |m, cx| {
                m.profiles = list(profiles);
                m.dashboard = list(dashboard);
                m.folders = list(folders);
                m.collapsed_folders = list(collapsed).iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                if let Ok(s) = settings {
                    m.settings = s;
                }
                m.apply_palette(cx);
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
        self.toast = Some(Toast { title: title.into(), body: body.into(), error });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |m, cx| {
                m.toast = None;
                cx.notify();
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
            let _ = this.update(cx, |m, cx| {
                m.refreshing.remove(&id);
                match result {
                    Ok(v) if v.get("success") == Some(&Value::Bool(false)) => {
                        let e = v.get("error").and_then(Value::as_str).unwrap_or("Refresh failed").to_string();
                        m.show_toast("Refresh failed", e, true, cx);
                    }
                    Ok(_) => {}
                    Err(e) => m.show_toast("Refresh failed", e, true, cx),
                }
                m.reload(cx);
            });
        })
        .detach();
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

    /// Save one settings key (theme, style...) and apply it.
    pub fn set_setting(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        if let Some(obj) = self.settings.as_object_mut() {
            obj.insert(key.to_string(), value);
        }
        self.apply_palette(cx);
        cx.notify();
        let settings = self.settings.clone();
        self.run("settings:save", vec![settings], cx);
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
            "auth:credentialsRefreshed" | "auth:credentialsExpired" | "auth:networkUnavailable" | "scheduler:pausedChanged" => {
                if let Some(id) = &first {
                    self.refreshing.remove(id);
                }
                self.reload(cx);
            }
            "notify" => {
                let body = event.payload.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                self.show_toast(first.unwrap_or_default(), body, false, cx);
            }
            "auth:credentialsRequired" | "auth:ssoLoginRequired" => {
                if let Some(id) = &first {
                    self.refreshing.remove(id);
                }
                // TODO(phase 4, modals): the credentials dialog and SSO prompt.
                self.show_toast("Sign-in needed", "Refresh the profile to sign in.", false, cx);
            }
            _ => {}
        }
    }
}

fn list(v: Result<Value, String>) -> Vec<Value> {
    match v {
        Ok(Value::Array(a)) => a,
        _ => Vec::new(),
    }
}
