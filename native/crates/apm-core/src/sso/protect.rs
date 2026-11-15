//! The outer protection of `sso-sessions.json` values (docs/native-rewrite-plan.md §6.3).
//!
//! 1.x wrapped every value with Electron `safeStorage`, tied to the OS user:
//! - Windows: base64( "v10" | nonce(12) | AES-256-GCM ciphertext+tag ). The key is
//!   `os_crypt.encrypted_key` in Electron's `Local State` (base64 of "DPAPI" + a DPAPI blob), under
//!   Electron's userData, `%APPDATA%\aws-profile-manager` (named after package.json "name", not the
//!   product name, so it sits next to the app-data directory). Verified by spike (g).
//! - macOS: base64( "v10" | AES-128-CBC ciphertext ), key = PBKDF2-SHA1(password, "saltysalt",
//!   1003 rounds, 16 bytes), IV = 16 spaces; the password is the "AWS Profile Manager Safe Storage"
//!   Keychain item. Not yet verified against a real 1.x install (spike (h) runs on the Mac).
//!
//! 2.0 reads those values but writes its own: `apm2:` + base64 of
//! - Windows: a DPAPI blob of the inner value (no key file to lose or migrate);
//! - elsewhere: nonce(12) | AES-256-GCM ciphertext+tag with a random key held in the keychain
//!   (account [`STORE_KEY_ACCOUNT`] under the app's service).
//!
//! There is deliberately no plaintext fallback, as in 1.x: if protection is unavailable the
//! session lives in memory only.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use crate::secrets::SecretStore;

/// Prefix of values written by 2.0.
pub const V2_PREFIX: &str = "apm2:";
/// Keychain account of the 2.0 store key (not used on Windows, where DPAPI needs no key).
pub const STORE_KEY_ACCOUNT: &str = "__sso_session_store_key__";
/// Electron's userData directory name (package.json "name").
const ELECTRON_USER_DATA: &str = "aws-profile-manager";
/// Electron names the macOS Keychain item after the product name.
const MAC_SAFE_STORAGE_SERVICE: &str = "AWS Profile Manager Safe Storage";
const MAC_SAFE_STORAGE_ACCOUNTS: &[&str] = &["AWS Profile Manager Key", "AWS Profile Manager"];

/// Where 1.x Electron kept `Local State`: a sibling of the app-data directory.
pub fn electron_user_data(app_data: &Path) -> PathBuf {
    app_data.parent().map(|p| p.join(ELECTRON_USER_DATA)).unwrap_or_else(|| PathBuf::from(ELECTRON_USER_DATA))
}

// ------------------------------------------------------------------------------------------------
// 2.0 values
// ------------------------------------------------------------------------------------------------

/// Protect an inner value for the 2.0 store. `None` = protection unavailable (no plaintext fallback).
pub fn wrap(inner: &str, secrets: &dyn SecretStore) -> Option<String> {
    let bytes = wrap_bytes(inner.as_bytes(), secrets)?;
    Some(format!("{V2_PREFIX}{}", B64.encode(bytes)))
}

#[cfg(windows)]
fn wrap_bytes(plain: &[u8], _secrets: &dyn SecretStore) -> Option<Vec<u8>> {
    dpapi::protect(plain)
}

#[cfg(not(windows))]
fn wrap_bytes(plain: &[u8], secrets: &dyn SecretStore) -> Option<Vec<u8>> {
    let key = store_key(secrets, true)?;
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).ok()?;
    let nonce: [u8; 12] = rand::random();
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(aes_gcm::Nonce::from_slice(&nonce), plain).ok()?);
    Some(out)
}

#[cfg(not(windows))]
fn store_key(secrets: &dyn SecretStore, create: bool) -> Option<Vec<u8>> {
    if let Some(k) = secrets.get(STORE_KEY_ACCOUNT).and_then(|s| B64.decode(s).ok()).filter(|k| k.len() == 32) {
        return Some(k);
    }
    if !create {
        return None;
    }
    let key: [u8; 32] = rand::random();
    secrets.set(STORE_KEY_ACCOUNT, &B64.encode(key)).ok()?;
    Some(key.to_vec())
}

fn unwrap_v2(encoded: &str, secrets: &dyn SecretStore) -> Option<String> {
    let bytes = B64.decode(encoded).ok()?;
    #[cfg(windows)]
    let plain = {
        let _ = secrets;
        dpapi::unprotect(&bytes)?
    };
    #[cfg(not(windows))]
    let plain = {
        if bytes.len() < 12 {
            return None;
        }
        let key = store_key(secrets, false)?;
        let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).ok()?;
        let (nonce, ct) = bytes.split_at(12);
        cipher.decrypt(aes_gcm::Nonce::from_slice(nonce), ct).ok()?
    };
    String::from_utf8(plain).ok()
}

/// The inner value of a stored entry, written by 2.0 or by 1.x. `None` when it cannot be read
/// (another OS user, a reinstalled key, a missing `Local State`): treated as absent, never fatal.
pub fn unwrap(encoded: &str, app_data: &Path, secrets: &dyn SecretStore) -> Option<String> {
    match encoded.strip_prefix(V2_PREFIX) {
        Some(rest) => unwrap_v2(rest, secrets),
        None => unwrap_safe_storage(encoded, app_data),
    }
}

// ------------------------------------------------------------------------------------------------
// 1.x Electron safeStorage values (read only)
// ------------------------------------------------------------------------------------------------

fn unwrap_safe_storage(encoded: &str, app_data: &Path) -> Option<String> {
    let blob = B64.decode(encoded.trim()).ok()?;
    let body = blob.strip_prefix(b"v10")?;
    let plain = if cfg!(target_os = "macos") {
        decrypt_mac_v10(body, &mac_safe_storage_password()?)?
    } else {
        decrypt_windows_v10(body, &windows_local_state_key(app_data)?)?
    };
    String::from_utf8(plain).ok()
}

/// AES-256-GCM `v10` body (after the prefix): nonce(12) | ciphertext+tag.
pub fn decrypt_windows_v10(body: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    if body.len() < 12 + 16 {
        return None;
    }
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(key).ok()?;
    let (nonce, ct) = body.split_at(12);
    cipher.decrypt(aes_gcm::Nonce::from_slice(nonce), ct).ok()
}

/// AES-128-CBC `v10` body (after the prefix), Chromium's macOS scheme.
pub fn decrypt_mac_v10(body: &[u8], password: &str) -> Option<Vec<u8>> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), b"saltysalt", 1003, &mut key);
    let iv = [b' '; 16];
    cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into()).decrypt_padded_vec_mut::<Pkcs7>(body).ok()
}

/// The safeStorage key from Electron's `Local State` (Windows).
fn windows_local_state_key(app_data: &Path) -> Option<Vec<u8>> {
    let path = electron_user_data(app_data).join("Local State");
    let ls: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let enc = B64.decode(ls.get("os_crypt")?.get("encrypted_key")?.as_str()?).ok()?;
    let enc = enc.strip_prefix(b"DPAPI")?;
    #[cfg(windows)]
    {
        dpapi::unprotect(enc)
    }
    #[cfg(not(windows))]
    {
        let _ = enc;
        None
    }
}

fn mac_safe_storage_password() -> Option<String> {
    MAC_SAFE_STORAGE_ACCOUNTS.iter().find_map(|account| keyring::Entry::new(MAC_SAFE_STORAGE_SERVICE, account).ok()?.get_password().ok())
}

// ------------------------------------------------------------------------------------------------
// DPAPI
// ------------------------------------------------------------------------------------------------

#[cfg(windows)]
pub mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    fn call(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: input points at `data` for the duration of the call; on success the output buffer
        // is allocated by the system, copied, then released with LocalFree.
        let ok = unsafe {
            if protect {
                CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            }
        };
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        unsafe { LocalFree(out.pbData as _) };
        Some(v)
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        call(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        call(data, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemSecrets(Mutex<std::collections::HashMap<String, String>>);
    impl SecretStore for MemSecrets {
        fn get(&self, a: &str) -> Option<String> {
            self.0.lock().unwrap().get(a).cloned()
        }
        fn set(&self, a: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(a.into(), v.into());
            Ok(())
        }
        fn delete(&self, a: &str) {
            self.0.lock().unwrap().remove(a);
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("apm-sso-protect-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("AWSProfileManager")).unwrap();
        d
    }

    #[test]
    fn v2_round_trip() {
        let dir = temp_dir("v2");
        let secrets = MemSecrets::default();
        let wrapped = wrap("v1:inner", &secrets).unwrap();
        assert!(wrapped.starts_with(V2_PREFIX));
        assert!(!wrapped.contains("inner"));
        assert_eq!(unwrap(&wrapped, &dir.join("AWSProfileManager"), &secrets).as_deref(), Some("v1:inner"));
        assert_eq!(unwrap("apm2:bm90IHZhbGlk", &dir.join("AWSProfileManager"), &secrets), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A 1.x value as Electron writes it on Windows, with a Local State next to the app data.
    #[cfg(windows)]
    #[test]
    fn reads_electron_safe_storage_on_windows() {
        let dir = temp_dir("v10");
        let key: [u8; 32] = rand::random();
        let blob = dpapi::protect(&key).unwrap();
        let mut enc = b"DPAPI".to_vec();
        enc.extend(blob);
        let user_data = dir.join("aws-profile-manager");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(user_data.join("Local State"), serde_json::json!({ "os_crypt": { "encrypted_key": B64.encode(enc) } }).to_string()).unwrap();
        let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce: [u8; 12] = rand::random();
        let mut value = b"v10".to_vec();
        value.extend(nonce);
        value.extend(cipher.encrypt(aes_gcm::Nonce::from_slice(&nonce), b"{\"identity\":\"a\"}".as_ref()).unwrap());
        let app_data = dir.join("AWSProfileManager");
        let secrets = MemSecrets::default();
        assert_eq!(unwrap(&B64.encode(&value), &app_data, &secrets).as_deref(), Some("{\"identity\":\"a\"}"));
        // Tampered or written under another key: unreadable, not an error.
        let last = value.len() - 1;
        value[last] ^= 1;
        assert_eq!(unwrap(&B64.encode(&value), &app_data, &secrets), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decrypts_chromium_mac_v10() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        let mut key = [0u8; 16];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"pw", b"saltysalt", 1003, &mut key);
        let ct = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(b"v1:abc");
        assert_eq!(decrypt_mac_v10(&ct, "pw").as_deref(), Some(b"v1:abc".as_ref()));
        assert_eq!(decrypt_mac_v10(&ct, "wrong"), None);
    }
}
