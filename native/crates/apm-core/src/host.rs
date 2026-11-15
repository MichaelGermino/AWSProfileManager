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
    /// The auth audit log changed (the log viewer window reloads).
    fn log_viewer_changed(&self) {}
    /// Open (or focus) the auth log viewer window.
    fn open_log_viewer(&self) {}
    /// Start a specific browser with a URL (`externalBrowser.ts` `launch`): the executable
    /// directly with the URL as one argument on Windows, `open -a <bundle> <url>` on macOS. Never
    /// through a shell (`cmd /c start` truncates a URL at its first '&').
    fn launch_browser(&self, exe: &str, url: &str) -> Result<(), String> {
        let mut cmd = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("open");
            c.arg("-a").arg(exe).arg(url);
            c
        } else {
            let mut c = std::process::Command::new(exe);
            c.arg(url);
            c
        };
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        cmd.spawn().map(drop).map_err(|e| e.to_string())
    }
    /// Open the AWS console in an in-app window (settings.consoleBrowserMode 'embedded'): 1400x900,
    /// titled `title`, with its own persistent storage per profile (1.x partition
    /// `persist:console-<profileId>`) so each account's console session stays independent. The
    /// default (no embedded browser) opens it in the default browser instead.
    fn open_console_window(&self, _profile_id: &str, _title: &str, url: &str) {
        self.open_external("url", url);
    }
    /// Show and focus the main window (1.x `mainWindow.show(); mainWindow.focus()` before asking
    /// for credentials or reporting expired credentials).
    fn show_main_window(&self) {}
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
