//! Updates. Port of the IPC-visible behaviour of `autoUpdater.ts` (electron-updater in 1.x).
//!
//! The window sees the same states as in 1.x, as the `update` event and from `update:getStatus`:
//! `available {version}`, `downloading {percent}`, `downloaded {version}`, `installing {version?}`,
//! `error {message}`, `no-update`.
//!
//! The check reads the GitHub releases of the repository (the same source electron-updater's
//! github provider used): the newest published release, pre-releases only when
//! settings.allowPrerelease is on, newer than the running version.
//!
//! TODO(updater, plan §7.2): download, verification and install are not ported yet. 2.0 replaces
//! electron-updater with its own: a `latest.json` feed (stable and pre-release), download with
//! `downloading` progress, ed25519 signature (`minisign-verify`) and sha256 checks, then on Windows
//! the NSIS installer run silently before exit, on macOS the `.app` swapped and relaunched. The
//! hourly check (first one 5 s after start) is the app's to schedule with
//! [`Core::check_for_updates_now`]. Until then a check stops at `available`, and an install request
//! reports the same error 1.x showed when the installer could not start.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::core::Core;
use crate::host::Host;

const CHANNEL: &str = "update";
const GITHUB_OWNER: &str = "MichaelGermino";
const GITHUB_REPO: &str = "AWSProfileManager";

/// How long the window gets to paint the "Installing update" overlay before the quit starts.
/// Quitting tears down the tray, the terminals and every window, and while that runs the window
/// cannot change — without a visible state first, it just looks frozen, and users clicked the
/// button again and again.
const PAINT_BEFORE_QUIT: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct UpdateState {
    /// `lastUpdateStatus`.
    pub last: Option<Value>,
    /// Repeat clicks while the quit is under way must not start a second install.
    pub install_requested: bool,
}

/// A semver-ish version: numeric core plus optional pre-release identifiers.
fn parse_version(v: &str) -> Option<(Vec<u64>, Vec<String>)> {
    let v = v.trim().trim_start_matches('v');
    let v = v.split('+').next()?;
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
        None => (v, Vec::new()),
    };
    let nums: Option<Vec<u64>> = core.split('.').map(|n| n.parse().ok()).collect();
    let mut nums = nums?;
    nums.resize(3, 0);
    Some((nums, pre))
}

/// Semver precedence: `a > b`.
pub fn is_newer(a: &str, b: &str) -> bool {
    let (Some((an, ap)), Some((bn, bp))) = (parse_version(a), parse_version(b)) else { return false };
    if an != bn {
        return an > bn;
    }
    match (ap.is_empty(), bp.is_empty()) {
        (true, true) => false,
        (true, false) => true,
        (false, true) => false,
        (false, false) => {
            for (x, y) in ap.iter().zip(&bp) {
                let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(xn), Ok(yn)) => xn.cmp(&yn),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord == std::cmp::Ordering::Greater;
                }
            }
            ap.len() > bp.len()
        }
    }
}

fn send(state: &std::sync::Mutex<UpdateState>, host: &dyn Host, payload: Value) {
    state.lock().unwrap().last = Some(payload.clone());
    host.emit(CHANNEL, vec![payload]);
}

impl Core {
    /// `getLastUpdateStatus`.
    pub fn last_update_status(&self) -> Option<Value> {
        self.extras.update.lock().unwrap().last.clone()
    }

    async fn newest_release(&self) -> Result<Option<String>, String> {
        let allow_prerelease = self.settings().get().get("allowPrerelease") == Some(&Value::Bool(true));
        let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases?per_page=30");
        let res = self
            .cfg
            .http
            .client(false, true)
            .get(self.cfg.http.remap(&url))
            .header("accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        if status != 200 {
            return Err(format!("Update check failed (HTTP {status})."));
        }
        let list: Value = res.json().await.map_err(|e| e.to_string())?;
        let mut best: Option<String> = None;
        for r in list.as_array().into_iter().flatten() {
            if r.get("draft") == Some(&Value::Bool(true)) {
                continue;
            }
            if r.get("prerelease") == Some(&Value::Bool(true)) && !allow_prerelease {
                continue;
            }
            let Some(tag) = r.get("tag_name").and_then(Value::as_str) else { continue };
            let version = tag.trim_start_matches('v').to_string();
            if parse_version(&version).is_some() && best.as_deref().is_none_or(|b| is_newer(&version, b)) {
                best = Some(version);
            }
        }
        Ok(best)
    }

    /// `checkForUpdatesNow`: check, notify the window, and resolve with the result (no-update,
    /// available, or error).
    pub async fn check_for_updates_now(&self) -> Value {
        let payload = match self.newest_release().await {
            Ok(Some(v)) if is_newer(&v, &self.cfg.app_version) => json!({ "type": "available", "version": v }),
            Ok(_) => json!({ "type": "no-update" }),
            Err(message) => {
                let message = if message.is_empty() { "Update check failed".to_string() } else { message };
                json!({ "type": "error", "message": message })
            }
        };
        send(&self.extras.update, self.host.as_ref(), payload.clone());
        payload
    }

    /// `installUpdateAndRestart`.
    pub fn install_update_and_restart(&self) {
        let state = Arc::clone(&self.extras.update);
        let version = {
            let mut s = state.lock().unwrap();
            if s.install_requested {
                return;
            }
            s.install_requested = true;
            match &s.last {
                Some(v) if v.get("type").and_then(Value::as_str) == Some("downloaded") => v.get("version").cloned(),
                _ => None,
            }
        };
        let mut installing = json!({ "type": "installing" });
        if let Some(v) = version {
            installing["version"] = v;
        }
        send(&state, self.host.as_ref(), installing);

        let host = self.host.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PAINT_BEFORE_QUIT).await;
            // TODO(updater, plan §7.2): run the downloaded, verified installer and quit. Nothing is
            // downloaded yet, so this is the path 1.x took when the installer could not be started:
            // allow another attempt and tell the window.
            state.lock().unwrap().install_requested = false;
            send(
                &state,
                host.as_ref(),
                json!({ "type": "error", "message": "The update could not be installed. Check for updates to download it again." }),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn semver_precedence() {
        assert!(is_newer("1.6.2", "1.6.1"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.6.1", "1.6.1"));
        assert!(is_newer("2.0.0", "2.0.0-beta.2"));
        assert!(is_newer("2.0.0-beta.10", "2.0.0-beta.2"));
        assert!(!is_newer("2.0.0-alpha", "2.0.0-beta"));
        assert!(!is_newer("junk", "1.0.0"));
    }
}
