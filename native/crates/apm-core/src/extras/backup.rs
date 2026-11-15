//! Config backup and restore. Port of `configBackup.ts`.
//!
//! A backup is the user's settings, profiles and folders. Secrets in the keychain (saved IdP
//! credentials, the master-password verifier) and the SSO session store are never part of it.
//! The Open WebUI API key lives in settings.json and is carried, as in 1.x, but it never reaches
//! the window: `config:restore` replaces it with `openWebUiApiKeySet` and keeps the key here until
//! `config:applyRestore` puts it back.

use serde_json::{json, Value};

use super::{canceled, failure, json_dialog};
use crate::core::Core;
use crate::json as jsjson;
use crate::store::Obj;

/// 2 added `folders`. A version 1 file restores fine — it simply has none, and since its profiles
/// carry no folderId either, the result is a correctly ungrouped list.
///
/// The reverse is also safe: an older build reading a version 2 file ignores `folders` and writes
/// the profiles back with folderId intact, which this version then renders as ungrouped until the
/// folders are recreated.
const BACKUP_VERSION: u64 = 2;

impl Core {
    /// `backupConfig`.
    pub fn backup_config(&self) -> Value {
        let Some(path) = self.host.dialog(json_dialog(true, "Save config backup", Some("aws-profile-manager-backup.json")))
        else {
            return canceled();
        };
        let objects = |list: Vec<Obj>| Value::Array(list.into_iter().map(Value::Object).collect());
        let data = json!({
            "version": BACKUP_VERSION,
            "settings": Value::Object(self.settings().get()),
            "profiles": objects(self.profiles().all()),
            "folders": objects(self.folders().all()),
        });
        match std::fs::write(&path, jsjson::stringify_pretty(&data)) {
            Ok(()) => json!({ "success": true, "path": path }),
            Err(e) => failure(e.to_string()),
        }
    }

    /// `restoreConfig`: validates the file and returns it for confirmation; applies nothing.
    pub fn restore_config(&self) -> Value {
        let Some(path) = self.host.dialog(json_dialog(false, "Restore from backup", None)) else {
            return canceled();
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(e) => return failure(e.to_string()),
        };
        let data: Value = match serde_json::from_str(&raw) {
            Ok(d) => d,
            Err(e) => return failure(e.to_string()),
        };
        let Some(profiles) = data.get("profiles").and_then(Value::as_array) else {
            return failure("Invalid backup file: missing or invalid profiles.");
        };
        let Some(settings) = data.get("settings").and_then(Value::as_object) else {
            return failure("Invalid backup file: missing or invalid settings.");
        };

        // A version 1 backup has no folders, and restore is replace-all: keeping the current ones
        // would leave folders that nothing in the restored set belongs to.
        let folders = data.get("folders").filter(|f| f.is_array()).cloned().unwrap_or_else(|| json!([]));

        // The API key stays in the core while the user confirms; applyRestore puts it back.
        let mut visible = settings.clone();
        let key = visible.shift_remove("openWebUiApiKey");
        let key_set = key.as_ref().and_then(Value::as_str).is_some_and(|k| !crate::js::trim(k).is_empty());
        *self.extras.pending_restore_api_key.lock().unwrap() = Some(key);
        visible.insert("openWebUiApiKeySet".into(), key_set.into());
        json!({
            "confirm": true,
            "settings": Value::Object(visible),
            "profiles": profiles,
            "folders": folders,
        })
    }

    /// `applyRestore`: replace settings, profiles and folders.
    pub fn apply_restore(&self, settings: Obj, profiles: &[Value], folders: &[Value]) -> Value {
        // The window never sees the backup's API key (restoreConfig kept it here). Restore replaces
        // everything, so the result carries exactly the backup's key, or none if it had none.
        let mut restored = settings;
        restored.shift_remove("openWebUiApiKeySet");
        let pending = self.extras.pending_restore_api_key.lock().unwrap().take();
        if !restored.contains_key("openWebUiApiKey") {
            if let Some(Some(key)) = pending {
                restored.insert("openWebUiApiKey".into(), key);
            }
        }
        self.settings().save(&restored);
        self.profiles().replace_all(profiles);
        // Folders after profiles, matching deleteFolder's ordering: if this fails, the worst case
        // is profiles carrying folderIds with no folders, which renders as ungrouped rather than lost.
        self.folders().replace_all(folders);
        self.host.tray_changed();
        json!({ "success": true })
    }
}
