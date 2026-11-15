//! What the core asks of the app around it: OS integration and the window. The GPUI app provides
//! the real implementation (apm-platform, apm-ui); the scenario runner provides a recording fake,
//! the way the 1.x suite stubbed `dialog` and `shell.openExternal`.

use serde_json::Value;

/// Native file dialog request (open or save).
#[derive(Debug, Clone)]
pub struct DialogRequest {
    pub save: bool,
    pub title: String,
    pub default_name: Option<String>,
    /// (label, extensions without dot)
    pub filters: Vec<(String, Vec<String>)>,
}

pub trait Host: Send + Sync {
    /// An event for the window (`IPC_EVENTS` in 1.x): channel plus positional payload.
    fn emit(&self, channel: &str, payload: Vec<Value>);
    /// Open a URL in the browser (`kind` "url") or a file with its default app (`kind` "path").
    fn open_external(&self, kind: &str, target: &str);
    /// Show a native open/save dialog. `None` = cancelled.
    fn dialog(&self, request: DialogRequest) -> Option<String>;
    /// Profiles, folders or lock state changed: rebuild the tray menu.
    fn tray_changed(&self) {}
    /// The launch-at-startup setting was saved.
    fn apply_launch_at_startup(&self, _enabled: bool) {}
    /// App icon, tray style or badge setting changed.
    fn app_icon_changed(&self) {}
}

/// A host that does nothing (headless use, unit tests).
pub struct NullHost;

impl Host for NullHost {
    fn emit(&self, _channel: &str, _payload: Vec<Value>) {}
    fn open_external(&self, _kind: &str, _target: &str) {}
    fn dialog(&self, _request: DialogRequest) -> Option<String> {
        None
    }
}
