//! Phase 0 spike (g): read 1.x `sso-sessions.json` values that Electron `safeStorage` encrypted.
//! Windows format: base64( "v10" | nonce(12) | AES-256-GCM ciphertext+tag ), key = DPAPI-unprotected
//! `os_crypt.encrypted_key` from Electron's `Local State` (minus the "DPAPI" prefix).
//! Prints only shapes and lengths — never a decrypted value.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::path::PathBuf;

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let mut input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(&mut input, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut out)
    };
    if ok == 0 {
        return Err("CryptUnprotectData failed".into());
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Ok(v)
}

fn main() -> Result<(), String> {
    let appdata = PathBuf::from(std::env::var("APPDATA").map_err(|e| e.to_string())?);
    // Electron's userData is named after package.json "name", not productName.
    let local_state = appdata.join("aws-profile-manager").join("Local State");
    let sessions = appdata.join("AWSProfileManager").join("sso-sessions.json");

    let ls: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&local_state).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let enc_key = B64.decode(ls["os_crypt"]["encrypted_key"].as_str().ok_or("no os_crypt.encrypted_key")?).map_err(|e| e.to_string())?;
    let enc_key = enc_key.strip_prefix(b"DPAPI").ok_or("key lacks DPAPI prefix")?;
    let key = dpapi_unprotect(enc_key)?;
    println!("Local State key: {} bytes", key.len());
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;

    let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&sessions).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let obj = file.as_object().ok_or("sso-sessions.json is not an object")?;
    let (mut ok, mut bad) = (0, 0);
    for (name, value) in obj {
        let label: String = name.chars().take(6).collect();
        let Some(s) = value.as_str() else { println!("{label}…: not a string"); bad += 1; continue };
        let blob = B64.decode(s).map_err(|e| e.to_string())?;
        let Some(rest) = blob.strip_prefix(b"v10") else { println!("{label}…: no v10 prefix"); bad += 1; continue };
        let (nonce, ct) = rest.split_at(12);
        match cipher.decrypt(Nonce::from_slice(nonce), ct) {
            Ok(plain) => {
                let kind = if plain.starts_with(b"v1:") { "master-password v1: envelope" } else if plain.starts_with(b"{") { "JSON" } else { "other" };
                println!("{label}…: decrypted {} bytes ({kind})", plain.len());
                ok += 1;
            }
            Err(_) => { println!("{label}…: AES-GCM decrypt failed"); bad += 1; }
        }
    }
    println!("result: {ok} decrypted, {bad} failed");
    if bad > 0 { Err("some values failed".into()) } else { Ok(()) }
}
