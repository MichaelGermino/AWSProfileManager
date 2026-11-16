//! Backup and restore, org config file, release notes, tour, console sign-in, browsers, app
//! icons, background video, updater. Ports of the matching 1.x services:
//!
//! | 1.x (`src/main/services/`)                              | here           |
//! |---------------------------------------------------------|----------------|
//! | `configBackup.ts`                                       | [`backup`]     |
//! | `orgConfigFile.ts`                                      | [`org_file`]   |
//! | `changelogService.ts`, `tourService.ts`, `devFlags.ts`  | [`whats_new`]  |
//! | `consoleSignIn.ts`                                      | [`console`]    |
//! | `externalBrowser.ts`                                    | [`browsers`]   |
//! | `appIcon.ts` (selection, previews)                      | [`icons`]      |
//! | `backgroundMedia.ts` (choose / clear)                   | [`background`] |
//! | `autoUpdater.ts`                                        | [`updater`]    |
//!
//! The IPC handler wrappers (`src/main/ipcHandlers.ts`) are in [`Core::dispatch_extras`], including
//! what a handler does besides the service call (reapplying the app icon after a restore).

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::core::{arg, arg_str, Core, DispatchResult};
use crate::host::DialogRequest;

pub mod background;
pub mod backup;
pub mod browsers;
pub mod console;
pub mod icons;
pub mod org_file;
pub mod updater;
pub mod whats_new;

/// Session state of this area (module-level `let`s in 1.x).
#[derive(Default)]
pub struct ExtrasState {
    /// configBackup.ts `pendingRestoreApiKey`: the API key of the backup awaiting confirmation
    /// (never sent to the window). Outer `None` = nothing pending; inner = the backup's
    /// `openWebUiApiKey` value, `None` when the backup had none (`undefined`).
    pub(crate) pending_restore_api_key: Mutex<Option<Option<Value>>>,
    /// autoUpdater.ts `lastUpdateStatus` and `installRequested`; shared with background tasks.
    pub(crate) update: Arc<Mutex<updater::UpdateState>>,
}

/// Where releases are published (update checks, release notes).
pub(crate) const GITHUB_OWNER: &str = "MichaelGermino";
pub(crate) const GITHUB_REPO: &str = "AWSProfileManager";

impl Core {
    /// A GET on this app's GitHub releases API; `rest` follows `/releases` (a query or a path).
    /// GitHub rejects requests without a User-Agent; the shared client always sends one.
    pub(crate) fn github_releases(&self, rest: &str) -> reqwest::RequestBuilder {
        let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases{rest}");
        self.cfg.http.client(false, true).get(self.cfg.http.remap(&url)).header("accept", "application/vnd.github+json")
    }
}

/// `{ success: false, error }`.
pub(crate) fn failure(error: impl Into<String>) -> Value {
    json!({ "success": false, "error": error.into() })
}

pub(crate) fn canceled() -> Value {
    json!({ "canceled": true })
}

/// `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The JSON file dialog every open/save in this area uses.
pub(crate) fn json_dialog(save: bool, title: &str, default_name: Option<&str>) -> DialogRequest {
    DialogRequest {
        save,
        title: title.to_string(),
        default_name: default_name.map(str::to_string),
        filters: vec![("JSON".to_string(), vec!["json".to_string()])],
    }
}

impl Core {
    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_extras(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let ok = Ok(Value::Null);
        Some(match channel {
            // Backup and restore (configBackup.ts)
            "config:backup" => Ok(self.backup_config()),
            "config:restore" => Ok(self.restore_config()),
            "config:applyRestore" => {
                let settings = arg(args, 0).as_object().cloned().unwrap_or_default();
                let empty = Vec::new();
                let profiles = arg(args, 1).as_array().unwrap_or(&empty);
                let folders = arg(args, 2).as_array().unwrap_or(&empty);
                let result = self.apply_restore(settings, profiles, folders);
                // A backup can carry a different app icon or tray style.
                self.apply_app_icon_change();
                Ok(result)
            }

            // Organization configuration file (orgConfigFile.ts)
            "orgConfig:export" => Ok(self.export_org_config(arg(args, 0).as_str())),
            "orgConfig:import" => Ok(self.import_org_config()),

            // Release notes for the running version, shown once after an update.
            "changelog:getPending" => Ok(self.pending_changelog().await.unwrap_or(Value::Null)),
            "changelog:markSeen" => {
                self.mark_changelog_seen(arg_str(args, 0));
                ok
            }

            // Guided feature tour. Core tracks only which tours have been seen; the window owns the steps.
            "tour:getState" => Ok(self.tour_state()),
            "tour:markSeen" => {
                self.mark_tour_seen(arg_str(args, 0));
                ok
            }
            "tour:resetWhatsNew" => {
                self.reset_whats_new_state();
                ok
            }

            // Console sign-in and browsers
            "system:listBrowsers" => {
                Ok(Value::Array(browsers::list_installed(self.cfg.platform).into_iter().map(|b| json!({ "key": b.key, "name": b.name })).collect()))
            }
            "console:open" => self.open_console_for_profile(arg_str(args, 0)).await,
            "console:enableMultiSession" => Ok(self.open_multi_session_opt_in()),

            // App icon and background video
            "app:getIconDataUrl" => Ok(self.app_icon_data_url().map_or(Value::Null, Value::String)),
            "app:listIcons" => Ok(self.list_app_icons()),
            "appearance:chooseVideo" => Ok(self.choose_background_video()),
            "appearance:clearVideo" => {
                self.clear_background_video();
                ok
            }

            // Updates
            "update:checkNow" => Ok(self.check_for_updates_now().await),
            "update:getStatus" => Ok(self.last_update_status().unwrap_or(Value::Null)),
            "update:installAndRestart" => {
                self.install_update_and_restart();
                ok
            }

            _ => return None,
        })
    }

    /// main.ts `applyAppIconChange`: the host re-applies window, taskbar, Dock, tray and shortcut
    /// icons; the window gets the new title-bar image (`app:iconChanged`).
    pub fn apply_app_icon_change(&self) {
        self.host.app_icon_changed();
        self.host.emit("app:iconChanged", vec![self.app_icon_data_url().map_or(Value::Null, Value::String)]);
    }

    /// What the 1.x window did on every launch once it was unlocked, before any user action
    /// (`src/renderer/App.tsx`): ask for pending release notes and check the tours. On a fresh
    /// install both settle without the user: the running version's notes and every shipped tour
    /// are recorded as seen, so that user meets the setup wizard rather than "what's new". Call it
    /// when the main window starts.
    ///
    /// Only that bookkeeping is done here. Showing notes or a tour, and banking older tours after
    /// an update (every unseen tour but the newest), stays with the window, which owns the tour
    /// steps and calls `changelog:getPending` / `tour:markSeen` itself.
    pub fn window_started(&self) {
        // 1.x gates both checks on the master-password screen being passed.
        if self.is_locked() {
            return;
        }
        self.settle_whats_new_for_fresh_install();
    }
}
