//! Updates (plan §7.2). Replaces electron-updater with the app's own updater, keeping the states the
//! window already knows, sent as the `update` event and returned by `update:getStatus`:
//! `available {version}`, `downloading {percent}`, `downloaded {version}`, `installing {version?}`,
//! `error {message}`, `no-update`.
//!
//! - Check: the GitHub releases of the repository, newest published release; pre-releases only
//!   when settings.allowPrerelease is on. Newer than the running version means `available`.
//! - Download (automatic, as electron-updater's `autoDownload`): every 2.x release carries an
//!   `apm-update.json` asset naming, per platform, the installer URL, its sha256 and a minisign
//!   (ed25519) signature. The file is downloaded with progress, and installed only when both the
//!   hash and the signature check out against the public key compiled into the app. A release
//!   without the manifest (1.x, or a platform not built) stays at `available`; the user can still
//!   download it from the release page.
//! - Install: the host runs the verified installer and quits (Windows: NSIS silently; macOS: swap
//!   the `.app` and relaunch).
//! - Schedule: first check 5 s after start, then hourly ([`Core::start_update_checks`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::core::Core;
use crate::host::Host;

const CHANNEL: &str = "update";
/// The per-release manifest asset (see the module docs).
pub const MANIFEST_ASSET: &str = "apm-update.json";

const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

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
    /// Version being downloaded, so a second check does not start a second download.
    pub downloading: Option<String>,
    /// The verified installer, ready to run.
    pub downloaded: Option<(String, PathBuf)>,
    pub checks_started: bool,
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

/// The manifest key for the platform this binary was built for.
pub fn platform_key() -> &'static str {
    match (cfg!(windows), cfg!(target_os = "macos"), cfg!(target_arch = "aarch64")) {
        (true, _, false) => "windows-x86_64",
        (true, _, true) => "windows-aarch64",
        (_, true, true) => "darwin-aarch64",
        (_, true, false) => "darwin-x86_64",
        _ => "linux-x86_64",
    }
}

/// One platform's entry in `apm-update.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateAsset {
    pub url: String,
    pub sha256: String,
    /// The full minisign signature file (`.minisig`) for the installer.
    pub signature: String,
}

/// `{"version": "2.0.1", "platforms": {"windows-x86_64": {"url", "sha256", "signature"}, ...}}`.
pub fn parse_manifest(manifest: &Value, platform: &str) -> Option<(String, UpdateAsset)> {
    let version = manifest.get("version")?.as_str()?.to_string();
    let entry = manifest.get("platforms")?.get(platform)?;
    let s = |k: &str| entry.get(k).and_then(Value::as_str).map(str::to_string);
    Some((version, UpdateAsset { url: s("url")?, sha256: s("sha256")?.to_lowercase(), signature: s("signature")? }))
}

/// The downloaded bytes must match both the published sha256 and the minisign signature made
/// with the release key. Either check failing means the file is not installed.
pub fn verify_installer(bytes: &[u8], asset: &UpdateAsset, public_key: &str) -> Result<(), String> {
    let digest = hex::encode(Sha256::digest(bytes));
    if digest != asset.sha256 {
        return Err("The downloaded update is damaged (checksum mismatch).".into());
    }
    let pk = minisign_verify::PublicKey::decode(public_key.trim())
        .or_else(|_| minisign_verify::PublicKey::from_base64(public_key.trim()))
        .map_err(|_| "Updates are not set up for this build (no valid release key).".to_string())?;
    let sig = minisign_verify::Signature::decode(&asset.signature).map_err(|_| "The update's signature could not be read.".to_string())?;
    pk.verify(bytes, &sig, false).map_err(|_| "The update's signature is not valid.".to_string())
}

fn send(state: &std::sync::Mutex<UpdateState>, host: &dyn Host, payload: Value) {
    state.lock().unwrap().last = Some(payload.clone());
    host.emit(CHANNEL, vec![payload]);
}

struct Release {
    version: String,
    manifest_url: Option<String>,
}

impl Core {
    /// `getLastUpdateStatus`.
    pub fn last_update_status(&self) -> Option<Value> {
        self.extras.update.lock().unwrap().last.clone()
    }

    async fn newest_release(&self) -> Result<Option<Release>, String> {
        let allow_prerelease = self.settings().get().get("allowPrerelease") == Some(&Value::Bool(true));
        let res = self.github_releases("?per_page=30").send().await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        if status != 200 {
            return Err(format!("Update check failed (HTTP {status})."));
        }
        let list: Value = res.json().await.map_err(|e| e.to_string())?;
        let mut best: Option<Release> = None;
        for r in list.as_array().into_iter().flatten() {
            if r.get("draft") == Some(&Value::Bool(true)) {
                continue;
            }
            if r.get("prerelease") == Some(&Value::Bool(true)) && !allow_prerelease {
                continue;
            }
            let Some(tag) = r.get("tag_name").and_then(Value::as_str) else { continue };
            let version = tag.trim_start_matches('v').to_string();
            if parse_version(&version).is_none() || best.as_ref().is_some_and(|b| !is_newer(&version, &b.version)) {
                continue;
            }
            let manifest_url = r
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|a| a.get("name").and_then(Value::as_str) == Some(MANIFEST_ASSET))
                .and_then(|a| a.get("browser_download_url").and_then(Value::as_str))
                .map(str::to_string);
            best = Some(Release { version, manifest_url });
        }
        Ok(best)
    }

    /// `checkForUpdatesNow`: check, notify the window, and resolve with the result (no-update,
    /// available, or error). An available update then downloads in the background.
    pub async fn check_for_updates_now(&self) -> Value {
        let release = self.newest_release().await;
        let payload = match &release {
            Ok(Some(r)) if is_newer(&r.version, &self.cfg.app_version) => json!({ "type": "available", "version": r.version }),
            Ok(_) => json!({ "type": "no-update" }),
            Err(message) => {
                let message = if message.is_empty() { "Update check failed".to_string() } else { message.clone() };
                json!({ "type": "error", "message": message })
            }
        };
        // Already downloaded this version: say so again rather than "available".
        let ready = {
            let s = self.extras.update.lock().unwrap();
            match (&release, &s.downloaded) {
                (Ok(Some(r)), Some((v, _))) if *v == r.version => Some(v.clone()),
                _ => None,
            }
        };
        if let Some(v) = ready {
            let p = json!({ "type": "downloaded", "version": v });
            send(&self.extras.update, self.host.as_ref(), p.clone());
            return payload;
        }
        send(&self.extras.update, self.host.as_ref(), payload.clone());
        if let Ok(Some(r)) = release {
            if is_newer(&r.version, &self.cfg.app_version) {
                // The download outlives this call, so it needs the shared core (see `into_shared`).
                if let (Some(manifest_url), Some(core)) = (r.manifest_url, self.shared()) {
                    tokio::spawn(async move { core.download_update(r.version, manifest_url).await });
                }
            }
        }
        payload
    }

    /// Download and verify; `downloaded` when ready, `error` when anything fails.
    async fn download_update(&self, version: String, manifest_url: String) {
        {
            let mut s = self.extras.update.lock().unwrap();
            if s.downloading.as_deref() == Some(&version) {
                return;
            }
            s.downloading = Some(version.clone());
        }
        let result = self.download_and_verify(&version, &manifest_url).await;
        let mut s = self.extras.update.lock().unwrap();
        s.downloading = None;
        drop(s);
        match result {
            Ok(Some(path)) => {
                self.extras.update.lock().unwrap().downloaded = Some((version.clone(), path));
                send(&self.extras.update, self.host.as_ref(), json!({ "type": "downloaded", "version": version }));
            }
            // No build for this platform in the manifest: stay at `available`.
            Ok(None) => {}
            Err(message) => send(&self.extras.update, self.host.as_ref(), json!({ "type": "error", "message": message })),
        }
    }

    async fn download_and_verify(&self, version: &str, manifest_url: &str) -> Result<Option<PathBuf>, String> {
        let client = self.cfg.http.client(false, true);
        let manifest: Value = client
            .get(self.cfg.http.remap(manifest_url))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Update download failed: {e}"))?
            .json()
            .await
            .map_err(|_| "Update download failed: the release manifest is not valid.".to_string())?;
        let Some((manifest_version, asset)) = parse_manifest(&manifest, platform_key()) else { return Ok(None) };
        if manifest_version != version {
            return Err(format!("Update download failed: the release manifest is for {manifest_version}, not {version}."));
        }
        let res =
            client.get(self.cfg.http.remap(&asset.url)).send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("Update download failed: {e}"))?;
        let total = res.content_length();
        let mut bytes: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
        let mut stream = res.bytes_stream();
        let mut last_percent = -1.0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Update download failed: {e}"))?;
            bytes.extend_from_slice(&chunk);
            if let Some(t) = total.filter(|t| *t > 0) {
                let percent = (bytes.len() as f64 / t as f64 * 100.0).min(100.0);
                // At most one event per whole percent: the window redraws on each.
                if percent.floor() > last_percent {
                    last_percent = percent.floor();
                    send(&self.extras.update, self.host.as_ref(), json!({ "type": "downloading", "percent": percent }));
                }
            }
        }
        verify_installer(&bytes, &asset, self.cfg.update_public_key.as_deref().unwrap_or(""))?;
        let dir = self.cfg.paths.app_data.join("updates");
        let _ = std::fs::remove_dir_all(&dir); // only the newest download is ever kept
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = asset.url.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("update-installer");
        let path = dir.join(name);
        std::fs::write(&path, &bytes).map_err(|e| format!("Update download failed: {e}"))?;
        Ok(Some(path))
    }

    /// `installUpdateAndRestart`: hand the verified installer to the host, which runs it and quits.
    pub fn install_update_and_restart(&self) {
        let state = Arc::clone(&self.extras.update);
        let (version, installer) = {
            let mut s = state.lock().unwrap();
            if s.install_requested {
                return;
            }
            s.install_requested = true;
            match &s.downloaded {
                Some((v, p)) => (Some(Value::String(v.clone())), Some(p.clone())),
                None => (None, None),
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
            let result = match installer {
                Some(path) => host.install_update(&path),
                None => Err("no update downloaded".into()),
            };
            if result.is_err() {
                // The installer could not be started: allow another attempt and tell the window,
                // in 1.x's words.
                state.lock().unwrap().install_requested = false;
                send(
                    &state,
                    host.as_ref(),
                    json!({ "type": "error", "message": "The update could not be installed. Check for updates to download it again." }),
                );
            }
        });
    }

    /// First check 5 s after start, then hourly (installed builds only; the app decides).
    pub fn start_update_checks(self: &Arc<Self>) {
        {
            let mut s = self.extras.update.lock().unwrap();
            if s.checks_started {
                return;
            }
            s.checks_started = true;
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(INITIAL_CHECK_DELAY).await;
            let mut interval = tokio::time::interval(CHECK_INTERVAL);
            loop {
                interval.tick().await;
                let Some(core) = weak.upgrade() else { break };
                core.check_for_updates_now().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn signed(bytes: &[u8]) -> (String, UpdateAsset) {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let sig = minisign::sign(Some(&kp.pk), &kp.sk, std::io::Cursor::new(bytes), None, None).unwrap();
        let asset = UpdateAsset { url: "https://example.test/setup.exe".into(), sha256: hex::encode(Sha256::digest(bytes)), signature: sig.into_string() };
        (kp.pk.to_box().unwrap().to_string(), asset)
    }

    #[test]
    fn verifies_hash_and_signature() {
        let installer = b"pretend installer";
        let (pk, asset) = signed(installer);
        assert_eq!(verify_installer(installer, &asset, &pk), Ok(()));
        // Tampered bytes fail the hash first.
        assert!(verify_installer(b"tampered", &asset, &pk).unwrap_err().contains("checksum"));
        // Right hash, wrong key: the signature check catches a swapped release.
        let (other_pk, _) = signed(b"x");
        assert!(verify_installer(installer, &asset, &other_pk).unwrap_err().contains("signature"));
        // No key compiled in: never install.
        assert!(verify_installer(installer, &asset, "").unwrap_err().contains("not set up"));
    }

    #[test]
    fn manifest_picks_the_platform() {
        let m = json!({ "version": "2.0.1", "platforms": {
            "windows-x86_64": { "url": "https://x/AWS-Profile-Manager-Setup-2.0.1.exe", "sha256": "AB", "signature": "s" }
        }});
        let (v, a) = parse_manifest(&m, "windows-x86_64").unwrap();
        assert_eq!(v, "2.0.1");
        assert_eq!(a.sha256, "ab");
        assert!(parse_manifest(&m, "darwin-aarch64").is_none());
    }
}
