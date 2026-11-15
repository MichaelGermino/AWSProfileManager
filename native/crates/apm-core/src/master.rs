//! Master password and saved IdP credentials. Port of `credentialStorage.ts`.
//!
//! Keychain layout (shared with 1.x):
//! - `__default__`: the default IdP sign-in. With a master password it is `v1:` of
//!   `{"username","password"}`; legacy 1.x installs may hold the plain password with the username
//!   in `__default___username`.
//! - `__master_password_verifier__`: `v1:` of [`crypto::VERIFIER_PLAINTEXT`], so a password can
//!   always be checked even when nothing else is stored.
//! - `<profileId>` / `<profileId>_username`: legacy per-profile entries (not written by 1.6+).
//!
//! The master password itself is never stored; it lives in memory for the session.

use serde_json::{json, Value};

use crate::core::Core;
use crate::crypto::{self, VERIFIER_PLAINTEXT};
use crate::master_password;

pub const DEFAULT_CREDENTIALS_ID: &str = "__default__";
pub const VERIFIER_ID: &str = "__master_password_verifier__";
pub const MASTER_PASSWORD_REQUIRED: &str = "MASTER_PASSWORD_REQUIRED";

fn is_encrypted(v: Option<&str>) -> bool {
    v.is_some_and(crypto::is_v1)
}

fn username_account(id: &str) -> String {
    format!("{id}_username")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdpCredentials {
    pub username: String,
    pub password: String,
}

fn parse_creds(plain: &str) -> Option<IdpCredentials> {
    let v: Value = serde_json::from_str(plain).ok()?;
    let field = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(IdpCredentials { username: field("username"), password: field("password") })
}

fn ok() -> Value {
    json!({ "success": true })
}

fn fail(error: &str) -> Value {
    json!({ "success": false, "error": error })
}

impl Core {
    fn master_enabled(&self) -> bool {
        self.settings().get().get("masterPasswordEnabled") == Some(&Value::Bool(true))
    }

    fn set_master_enabled(&self, enabled: bool) {
        let mut s = self.settings().get();
        s.insert("masterPasswordEnabled".into(), enabled.into());
        self.settings().save(&s);
    }

    pub fn session_password(&self) -> Option<String> {
        self.session_master_password.lock().unwrap().clone()
    }

    fn profile_ids(&self) -> Vec<String> {
        self.profiles().all().iter().filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_string)).collect()
    }

    /// Locked = a master password is enabled and this session has not supplied it. Conservative on
    /// purpose (the tray uses it to gate actions).
    pub fn is_locked(&self) -> bool {
        self.master_enabled() && self.session_password().is_none()
    }

    /// Encrypt for storage when a master password is active; `Err(())` = locked (never downgrade
    /// to plaintext).
    pub fn protect_secret(&self, plaintext: &str) -> Result<String, ()> {
        if !self.master_enabled() {
            return Ok(plaintext.to_string());
        }
        let pw = self.session_password().ok_or(())?;
        Ok(crypto::encrypt(&pw, plaintext))
    }

    /// Inverse of [`Core::protect_secret`]. `None` when locked, wrong key or corrupt.
    pub fn unprotect_secret(&self, blob: &str) -> Option<String> {
        if !crypto::is_v1(blob) {
            return Some(blob.to_string());
        }
        crypto::decrypt(&self.session_password()?, blob)
    }

    /// `{needsUnlock}` / `{needsCreateMasterPassword}` / `{unlocked}` for the window.
    pub fn master_password_status(&self) -> Value {
        let secrets = &self.secrets;
        if self.master_enabled() {
            if self.session_password().is_some() {
                return json!({ "unlocked": true });
            }
            if is_encrypted(secrets.get(VERIFIER_ID).as_deref()) || is_encrypted(secrets.get(DEFAULT_CREDENTIALS_ID).as_deref()) {
                return json!({ "needsUnlock": true });
            }
            for id in self.profile_ids() {
                if is_encrypted(secrets.get(&id).as_deref()) {
                    return json!({ "needsUnlock": true });
                }
            }
            if self.sso_session_blobs().iter().any(|b| crypto::is_v1(b)) {
                return json!({ "needsUnlock": true });
            }
            // Nothing encrypted to unlock: clear the flag and let them in.
            self.set_master_enabled(false);
            return json!({ "unlocked": true });
        }
        let default_pass = secrets.get(DEFAULT_CREDENTIALS_ID);
        // Self-heal: ciphertext exists but the flag says no master password (settings reverted by
        // a restore or a hand edit). The ciphertext is the authority.
        let mut blobs = vec![default_pass.clone(), secrets.get(VERIFIER_ID)];
        let ids = self.profile_ids();
        blobs.extend(ids.iter().map(|id| secrets.get(id)));
        blobs.extend(self.sso_session_blobs().into_iter().map(Some));
        if blobs.iter().any(|b| is_encrypted(b.as_deref())) {
            self.set_master_enabled(true);
            return json!({ "needsUnlock": true });
        }
        if default_pass.as_deref().is_some_and(|p| !crypto::is_v1(p)) {
            return json!({ "needsCreateMasterPassword": true });
        }
        for id in ids {
            if secrets.get(&id).is_some_and(|v| !crypto::is_v1(&v)) {
                return json!({ "needsCreateMasterPassword": true });
            }
        }
        json!({ "unlocked": true })
    }

    /// Create the master password, encrypt a legacy plain default credential, drop SSO sessions
    /// (cheaper than re-encrypting; costs one sign-in), write the verifier, then set the flag.
    pub fn create_master_password(&self, password: &str, confirm: &str) -> Value {
        if password != confirm {
            return fail("Passwords do not match");
        }
        if let Some(err) = master_password::validate(password) {
            return fail(&err);
        }
        let secrets = &self.secrets;
        let default_pass = secrets.get(DEFAULT_CREDENTIALS_ID);
        let default_user = secrets.get(&username_account(DEFAULT_CREDENTIALS_ID));
        if (default_pass.is_some() || default_user.is_some()) && !is_encrypted(default_pass.as_deref()) {
            let plain = json!({ "username": default_user.unwrap_or_default(), "password": default_pass.unwrap_or_default() });
            if let Err(e) = secrets.set(DEFAULT_CREDENTIALS_ID, &crypto::encrypt(password, &crate::json::stringify(&plain))) {
                return fail(&e);
            }
            secrets.delete(&username_account(DEFAULT_CREDENTIALS_ID));
        }
        self.clear_all_sso_sessions();
        if let Err(e) = secrets.set(VERIFIER_ID, &crypto::encrypt(password, VERIFIER_PLAINTEXT)) {
            return fail(&e);
        }
        self.set_master_enabled(true);
        *self.session_master_password.lock().unwrap() = Some(password.to_string());
        ok()
    }

    /// Verify by decrypting one stored blob: the verifier first, then the default credential,
    /// legacy profile entries, and finally an SSO session.
    pub fn unlock(&self, password: &str) -> Value {
        let secrets = &self.secrets;
        let mut blob = secrets.get(VERIFIER_ID).filter(|b| crypto::is_v1(b));
        if blob.is_none() {
            blob = secrets.get(DEFAULT_CREDENTIALS_ID).filter(|b| crypto::is_v1(b));
        }
        if blob.is_none() {
            blob = self.profile_ids().iter().find_map(|id| secrets.get(id).filter(|b| crypto::is_v1(b)));
        }
        if blob.is_none() {
            blob = self.sso_session_blobs().into_iter().find(|b| crypto::is_v1(b));
        }
        let Some(blob) = blob else { return fail("No stored credentials to unlock") };
        if crypto::decrypt(password, &blob).is_none() {
            return fail("Wrong password");
        }
        *self.session_master_password.lock().unwrap() = Some(password.to_string());
        // Backfill the verifier for master passwords created before it existed.
        if !is_encrypted(secrets.get(VERIFIER_ID).as_deref()) {
            let _ = secrets.set(VERIFIER_ID, &crypto::encrypt(password, VERIFIER_PLAINTEXT));
        }
        ok()
    }

    /// Forget every saved secret and switch the master password off.
    pub fn forget_all_and_reset(&self) {
        *self.session_master_password.lock().unwrap() = None;
        self.set_master_enabled(false);
        let secrets = &self.secrets;
        for account in [VERIFIER_ID.to_string(), DEFAULT_CREDENTIALS_ID.to_string(), username_account(DEFAULT_CREDENTIALS_ID)] {
            secrets.delete(&account);
        }
        for id in self.profile_ids() {
            secrets.delete(&id);
            secrets.delete(&username_account(&id));
        }
        // Clearing the sign-in browser data is what forces a real re-authentication; the session
        // tokens go too.
        self.clear_all_auth_partitions();
        self.clear_all_sso_sessions();
    }

    /// Saved IdP sign-in for `profile_id` (only `__default__` is written by current builds).
    pub fn stored_credentials(&self, profile_id: &str) -> Option<IdpCredentials> {
        let raw = self.secrets.get(profile_id);
        if self.master_enabled() {
            let pw = self.session_password()?;
            if let Some(r) = raw.as_deref().filter(|r| crypto::is_v1(r)) {
                return crypto::decrypt(&pw, r).and_then(|plain| parse_creds(&plain));
            }
        }
        if is_encrypted(raw.as_deref()) {
            return None;
        }
        let username = self.secrets.get(&username_account(profile_id));
        if profile_id == DEFAULT_CREDENTIALS_ID {
            if username.is_none() && raw.is_none() {
                return None;
            }
            return Some(IdpCredentials { username: username.unwrap_or_default(), password: raw.unwrap_or_default() });
        }
        let raw = raw.filter(|r| !r.is_empty())?;
        Some(IdpCredentials { username: username.unwrap_or_default(), password: raw })
    }

    /// Default username for Settings (never the password); `{locked: true}` while locked.
    pub fn default_credentials_display(&self) -> Value {
        let secrets = &self.secrets;
        let plain_display = || {
            let username = secrets.get(&username_account(DEFAULT_CREDENTIALS_ID));
            let has_password = secrets.get(DEFAULT_CREDENTIALS_ID).is_some_and(|p| !p.is_empty());
            if username.is_none() && !has_password {
                Value::Null
            } else {
                json!({ "username": username.unwrap_or_default(), "hasPassword": has_password })
            }
        };
        if !self.master_enabled() {
            return plain_display();
        }
        let raw = secrets.get(DEFAULT_CREDENTIALS_ID);
        let Some(pw) = self.session_password() else {
            return if is_encrypted(raw.as_deref()) {
                json!({ "username": "", "hasPassword": true, "locked": true })
            } else {
                Value::Null
            };
        };
        match raw.filter(|r| crypto::is_v1(r)) {
            None => plain_display(),
            Some(r) => match crypto::decrypt(&pw, &r).and_then(|p| parse_creds(&p)) {
                Some(c) => json!({ "username": c.username, "hasPassword": !c.password.is_empty() }),
                None => Value::Null,
            },
        }
    }

    /// Save the default sign-in. Needs an unlocked master password; `password: None` keeps the
    /// saved one.
    pub fn set_default_credentials(&self, username: &str, password: Option<&str>) -> Value {
        let Some(pw) = self.session_password().filter(|_| self.master_enabled()) else {
            return json!({ "success": false, "error": MASTER_PASSWORD_REQUIRED });
        };
        let existing = self.stored_credentials(DEFAULT_CREDENTIALS_ID);
        let password = match (password, existing) {
            (Some(p), _) => p.to_string(),
            (None, Some(e)) => e.password,
            (None, None) => String::new(),
        };
        let payload = json!({ "username": username, "password": password });
        let _ = self.secrets.set(DEFAULT_CREDENTIALS_ID, &crypto::encrypt(&pw, &crate::json::stringify(&payload)));
        self.secrets.delete(&username_account(DEFAULT_CREDENTIALS_ID));
        Value::Null
    }

    pub fn delete_stored_credentials(&self, id: &str) {
        self.secrets.delete(id);
        self.secrets.delete(&username_account(id));
    }

    /// Per-profile IdP credentials are not used (only the default entry); kept for the contract.
    pub fn credentials_status(&self) -> Value {
        Value::Array(self.profile_ids().into_iter().map(|id| json!({ "profileId": id, "hasCredentials": false })).collect())
    }
}
