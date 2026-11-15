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

/// An embedded Identity Center sign-in (1.x: a `BrowserWindow` on a per-org persistent partition;
/// 2.0: a wry webview window owned by the UI, docs/native-rewrite-plan.md §3.4).
#[derive(Debug, Clone)]
pub struct SignInRequest {
    /// The authorize URL to load.
    pub url: String,
    /// The org's own browser data directory (cookies, IdP session). Persistent, so repeat sign-ins
    /// complete without input; isolated per org so a day-to-day browser identity is never reused.
    /// The core wipes it after a failed attempt and on sign-out, once the window is closed.
    pub data_dir: std::path::PathBuf,
    pub title: String,
}

/// The window of a running embedded sign-in. It starts HIDDEN: most re-authentications complete
/// on their own and the window is closed having never been seen.
pub trait SignInWindow: Send + Sync {
    /// Show and focus it (the IdP wants input, or a user joined a background attempt). Idempotent.
    fn reveal(&self) {}
    /// Close it (the sign-in finished or gave up). Must not count as the user closing it.
    fn close(&self) {}
}

/// A sign-in window that does nothing.
pub struct NoSignInWindow;

impl SignInWindow for NoSignInWindow {}

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
    /// The auth audit log changed (the log viewer window reloads).
    fn log_viewer_changed(&self) {}
    /// Open (or focus) the auth log viewer window.
    fn open_log_viewer(&self) {}
    /// Start an embedded sign-in window, hidden, loading `request.url`; the loopback listener in
    /// the core receives the redirect. Call `closed_by_user` if the user closes the window (that is
    /// how they say "not now"); a close through [`SignInWindow::close`] must not call it.
    /// The default has no embedded browser and reports the attempt as cancelled straight away.
    fn open_sign_in(&self, request: SignInRequest, closed_by_user: Box<dyn FnOnce() + Send>) -> Box<dyn SignInWindow> {
        let _ = request;
        closed_by_user();
        Box::new(NoSignInWindow)
    }
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
