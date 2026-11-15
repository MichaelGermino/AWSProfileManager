//! Cache of Identity Center OIDC state. Port of `ssoTokenStore.ts`.
//!
//! Keyed by org (start URL + region) rather than by profile: one SSO session serves every profile
//! in the org, unlike IdP credentials which are per-profile.
//!
//! ## Why this is not in the keychain
//!
//! Windows Credential Manager caps a credential blob at 2560 bytes (CRED_MAX_CREDENTIAL_BLOB_SIZE).
//! An SSO session carries an access token and a refresh token, each commonly 1–2 KB, plus the client
//! registration — and AES+base64 adds ~35% on top, so it does not fit.
//!
//! So sessions live in `sso-sessions.json` in app data: `{ "<sso:account>": "<protected value>" }`,
//! written as `JSON.stringify(map, null, 2)`. The inner value is the session JSON, wrapped in the
//! app's own `v1:` AES-256-GCM layer first when a master password is enabled; the outer protection
//! is OS-bound (see `protect.rs`: 1.x safeStorage values are read, 2.0 writes `apm2:` values and
//! replaces an org's 1.x value the next time that org's session is written). There is deliberately
//! **no plaintext fallback**: if protection is unavailable the session stays in memory only.
//!
//! Everything here is a bearer credential and must never reach the window.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::protect;
use crate::core::Core;
use crate::sso_org::{sso_keychain_account, SsoOrg};

pub const SESSIONS_FILENAME: &str = "sso-sessions.json";

/// Numbers written by JavaScript (`Date.now()`, epoch seconds) may come back as floats.
fn de_num<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_f64().map(|f| f as i64).unwrap_or(0))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoRegistration {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    /// Epoch seconds, from RegisterClient. Typically ~90 days out.
    #[serde(default, deserialize_with = "de_num")]
    pub client_secret_expires_at: i64,
    /// Registered redirect URI; pins the loopback port for subsequent logins.
    #[serde(default)]
    pub redirect_uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoToken {
    #[serde(default)]
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Epoch ms.
    #[serde(default, deserialize_with = "de_num")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SsoSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<SsoRegistration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<SsoToken>,
    /// Best-effort record of who signed in, so a wrong-account sign-in can be surfaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Fields a newer or older build wrote that this one does not know: kept on rewrite.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Why a session could not be persisted (it is still cached for this run).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPersisted {
    Locked,
    NoEncryption,
    WriteFailed,
}

impl NotPersisted {
    pub fn as_str(self) -> &'static str {
        match self {
            NotPersisted::Locked => "locked",
            NotPersisted::NoEncryption => "no-encryption",
            NotPersisted::WriteFailed => "write-failed",
        }
    }
}

/// Sessions also live in memory for the life of the process. Not an optimization: it keeps the
/// feature working when the session cannot be written (locked master password, no OS protection),
/// instead of silently falling back to a fresh interactive login on every call — which
/// re-registered the OIDC client and made AWS re-show its consent screen every time.
#[derive(Default)]
pub(crate) struct MemorySessions(Mutex<HashMap<String, SsoSession>>);

pub fn is_registration_usable(reg: Option<&SsoRegistration>, now_ms: i64) -> bool {
    let Some(reg) = reg else { return false };
    if reg.client_id.is_empty() || reg.client_secret.is_empty() || reg.redirect_uri.is_empty() {
        return false;
    }
    // Retire an hour early so a long-running refresh can't straddle the expiry.
    reg.client_secret_expires_at.saturating_mul(1000) > now_ms + 3_600_000
}

/// Access tokens last an hour; renew with a 5 minute margin so calls never race the expiry.
pub fn is_token_usable(token: Option<&SsoToken>, now_ms: i64) -> bool {
    token.is_some_and(|t| !t.access_token.is_empty() && t.expires_at > now_ms + 5 * 60_000)
}

impl Core {
    fn sessions_file(&self) -> PathBuf {
        self.cfg.paths.app_file(SESSIONS_FILENAME)
    }

    /// Missing or malformed — same contract as the other stores: treat as empty, never rewrite.
    fn read_session_map(&self) -> Map<String, Value> {
        match crate::json::read_file(&self.sessions_file()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        }
    }

    fn write_session_map(&self, map: &Map<String, Value>) -> bool {
        crate::json::write_file(&self.sessions_file(), &Value::Object(map.clone())).is_ok()
    }

    fn unwrap_stored(&self, encoded: &str) -> Option<String> {
        protect::unwrap(encoded, &self.cfg.paths.app_data, self.secrets.as_ref())
    }

    pub(crate) fn read_session(&self, org: &SsoOrg) -> Option<SsoSession> {
        let account = sso_keychain_account(org);
        let memory = &self.sso_state().memory.0;
        if let Some(s) = memory.lock().unwrap().get(&account) {
            return Some(s.clone());
        }
        let map = self.read_session_map();
        let stored = map.get(&account)?.as_str()?;
        let unwrapped = self.unwrap_stored(stored)?;
        // When a master password is enabled the inner value is `v1:` ciphertext; unprotect returns
        // None while locked, which correctly reads as "no session".
        let plain = self.unprotect_secret(&unwrapped)?;
        let session: SsoSession = serde_json::from_str(&plain).ok()?;
        memory.lock().unwrap().insert(account, session.clone());
        Some(session)
    }

    pub(crate) fn write_session(&self, org: &SsoOrg, session: &SsoSession) -> Result<(), NotPersisted> {
        let account = sso_keychain_account(org);
        // Cache first, unconditionally: a persistence failure must not cost the user their session.
        self.sso_state().memory.0.lock().unwrap().insert(account.clone(), session.clone());

        let json = serde_json::to_value(session).map(|v| crate::json::stringify(&v)).unwrap_or_default();
        let protected = self.protect_secret(&json).map_err(|_| NotPersisted::Locked)?;
        let wrapped = protect::wrap(&protected, self.secrets.as_ref()).ok_or(NotPersisted::NoEncryption)?;
        let mut map = self.read_session_map();
        map.insert(account, Value::String(wrapped));
        if self.write_session_map(&map) {
            Ok(())
        } else {
            Err(NotPersisted::WriteFailed)
        }
    }

    pub(crate) fn clear_session(&self, org: &SsoOrg) {
        let account = sso_keychain_account(org);
        self.sso_state().memory.0.lock().unwrap().remove(&account);
        let mut map = self.read_session_map();
        if map.remove(&account).is_some() {
            self.write_session_map(&map);
        }
    }

    /// Drop only the tokens, keeping the (still valid, consent-bearing) client registration.
    pub(crate) fn clear_token_keep_registration(&self, org: &SsoOrg) {
        let Some(session) = self.read_session(org) else { return };
        // Keep identity: it describes who signed in, not the expired session.
        let kept = SsoSession { registration: session.registration, identity: session.identity, ..Default::default() };
        let _ = self.write_session(org, &kept);
    }

    /// Drop every stored session. Used when the master password is created, changed, or reset.
    pub fn clear_all_sso_sessions(&self) {
        self.sso_state().memory.0.lock().unwrap().clear();
        // Best effort — a missing file is the desired end state anyway.
        let _ = std::fs::remove_file(self.sessions_file());
    }

    /// Inner (post-OS-protection) blobs of stored sessions, so the master-password checks can spot
    /// `v1:` ciphertext when deciding whether a master password needs unlocking and when verifying
    /// one. Returns ciphertext only — never a decrypted session.
    pub fn sso_session_blobs(&self) -> Vec<String> {
        self.read_session_map().values().filter_map(Value::as_str).filter_map(|v| self.unwrap_stored(v)).collect()
    }

    /// Account names of stored sessions (the sign-in browser data directories derive from them).
    /// Read them BEFORE clear_all_sso_sessions() deletes the file.
    pub(crate) fn stored_session_accounts(&self) -> Vec<String> {
        self.read_session_map().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_json_matches_1x_shape() {
        let s = SsoSession {
            registration: Some(SsoRegistration {
                client_id: "c".into(),
                client_secret: "s".into(),
                client_secret_expires_at: 1_900_000_000,
                redirect_uri: "http://127.0.0.1:54321/oauth/callback".into(),
            }),
            token: Some(SsoToken { access_token: "a".into(), refresh_token: None, expires_at: 1_800_000_000_000 }),
            identity: None,
            extra: Map::new(),
        };
        let text = crate::json::stringify(&serde_json::to_value(&s).unwrap());
        assert_eq!(
            text,
            r#"{"registration":{"clientId":"c","clientSecret":"s","clientSecretExpiresAt":1900000000,"redirectUri":"http://127.0.0.1:54321/oauth/callback"},"token":{"accessToken":"a","expiresAt":1800000000000}}"#
        );
        let back: SsoSession = serde_json::from_str(r#"{"token":{"accessToken":"a","expiresAt":1.8e12},"future":1}"#).unwrap();
        assert_eq!(back.token.unwrap().expires_at, 1_800_000_000_000);
        assert_eq!(back.extra.get("future"), Some(&Value::from(1)));
    }
}
