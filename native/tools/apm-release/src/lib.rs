//! Release tooling (plan §7, §8). Everything a release needs besides the build itself:
//!
//! - The updater key pair (minisign, ed25519): `keygen` writes the encrypted secret key and its
//!   password outside the repository and the public key into apm-core, which compiles it in.
//! - The update feeds for one built installer:
//!   - `apm-update.json`, the 2.0 updater's manifest, in exactly the shape
//!     `apm_core::extras::updater::parse_manifest` reads, with the installer's sha256 and a
//!     minisign signature that `verify_installer` checks against the compiled-in key;
//!   - `latest.yml` (plus `<channel>.yml` for a pre-release), the electron-updater feed that
//!     carries 1.x clients over to 2.0 (§7.1). electron-updater checks the sha512 and runs the
//!     file with `--updated` (and `/S` or `--force-run`).

use base64::Engine;
use minisign::{KeyPair, PublicKey, SecretKey, SecretKeyBox};
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha512};

pub const GITHUB_OWNER: &str = "MichaelGermino";
pub const GITHUB_REPO: &str = "AWSProfileManager";

/// The committed public key (the same file apm-core compiles in). Feeds are only written when the
/// secret key matches it, so a release can never be signed with a key the app does not trust.
pub const COMMITTED_PUBLIC_KEY: &str = include_str!("../../../crates/apm-core/src/update_key.pub");

/// The installer's file name on the release. No spaces: GitHub would turn them into dots, and the
/// name is also the URL in both feeds.
pub fn installer_file_name(version: &str) -> String {
    format!("AWS-Profile-Manager-Setup-{version}.exe")
}

/// Where GitHub serves a release asset.
pub fn github_download_url(version: &str, file: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/v{version}/{file}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// electron-updater's checksum: base64 of the raw SHA-512.
pub fn sha512_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
}

/// The pre-release identifier electron-updater derives its channel from (`2.0.0-beta.1` → `beta`).
pub fn prerelease_channel(version: &str) -> Option<String> {
    let pre = version.split('+').next()?.split_once('-')?.1;
    let first = pre.split('.').next()?;
    (!first.is_empty() && first.parse::<u64>().is_err()).then(|| first.to_string())
}

/// The electron-updater channel files to publish for a version: always `latest.yml`; for a
/// pre-release also `<channel>.yml`, which electron-updater asks for first when `allowPrerelease`
/// is on and the release tag carries that channel (it falls back to `latest.yml` otherwise).
pub fn channel_files(version: &str) -> Vec<String> {
    let mut files = vec!["latest.yml".to_string()];
    if let Some(ch) = prerelease_channel(version) {
        if ch != "latest" {
            files.push(format!("{ch}.yml"));
        }
    }
    files
}

/// The bridge feed in electron-builder's exact layout (as written for 1.x releases), with `size`.
/// `release_date` is an ISO-8601 timestamp with milliseconds, e.g. `2026-11-15T10:00:00.000Z`.
pub fn bridge_latest_yml(version: &str, file_name: &str, installer: &[u8], release_date: &str) -> String {
    let sha512 = sha512_base64(installer);
    format!(
        "version: {version}\nfiles:\n  - url: {file_name}\n    sha512: {sha512}\n    size: {size}\npath: {file_name}\nsha512: {sha512}\nreleaseDate: '{release_date}'\n",
        size = installer.len()
    )
}

/// A minisign signature file (`.minisig` text) over `bytes`, as `verify_installer` reads it.
pub fn sign(bytes: &[u8], sk: &SecretKey, trusted_comment: &str) -> Result<String, String> {
    let sig = minisign::sign(None, sk, std::io::Cursor::new(bytes), Some(trusted_comment), None).map_err(|e| e.to_string())?;
    Ok(sig.into_string())
}

/// One platform's entry for [`update_manifest`].
pub struct PlatformEntry {
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub signature: String,
}

impl PlatformEntry {
    pub fn new(platform: &str, url: &str, installer: &[u8], sk: &SecretKey, file_name: &str, version: &str) -> Result<Self, String> {
        let comment = format!("file:{file_name}\tversion:{version}");
        Ok(PlatformEntry { platform: platform.into(), url: url.into(), sha256: sha256_hex(installer), signature: sign(installer, sk, &comment)? })
    }
}

/// `apm-update.json`: `{"version", "platforms": {"<platform>": {"url", "sha256", "signature"}}}`.
/// `existing` (an earlier manifest of the same version, e.g. from another platform's job) is merged.
pub fn update_manifest(version: &str, entries: &[PlatformEntry], existing: Option<&Value>) -> Result<Value, String> {
    let mut platforms = serde_json::Map::new();
    if let Some(prev) = existing {
        if prev.get("version").and_then(Value::as_str) != Some(version) {
            return Err("the existing manifest is for a different version".into());
        }
        if let Some(p) = prev.get("platforms").and_then(Value::as_object) {
            platforms = p.clone();
        }
    }
    for e in entries {
        platforms.insert(e.platform.clone(), json!({ "url": e.url, "sha256": e.sha256, "signature": e.signature }));
    }
    Ok(json!({ "version": version, "platforms": platforms }))
}

/// A secret key file (minisign format) and its password.
pub fn load_secret_key(text: &str, password: &str) -> Result<SecretKey, String> {
    let sk_box = SecretKeyBox::from_string(text.trim()).map_err(|e| e.to_string())?;
    SecretKey::from_box(sk_box, Some(password.to_string())).map_err(|e| format!("cannot open the updater key: {e}"))
}

/// The public key of a secret key, as the base64 line of a `.pub` file.
pub fn public_key_base64(sk: &SecretKey) -> Result<String, String> {
    Ok(PublicKey::from_secret_key(sk).map_err(|e| e.to_string())?.to_base64())
}

/// The base64 line of a minisign `.pub` file (the line after the comment).
pub fn public_key_line(pub_file: &str) -> Option<&str> {
    pub_file.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
}

/// A new encrypted key pair: (secret key file text, public key file text).
pub fn generate_keypair(password: &str) -> Result<(String, String), String> {
    let kp = KeyPair::generate_encrypted_keypair(Some(password.to_string())).map_err(|e| e.to_string())?;
    let sk = kp.sk.to_box(Some("AWS Profile Manager updater secret key")).map_err(|e| e.to_string())?.into_string();
    let pk = kp.pk.to_box().map_err(|e| e.to_string())?.into_string();
    Ok((sk, pk))
}

/// A random password for the secret key (the key file is useless without it).
pub fn random_password() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..40).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels() {
        assert_eq!(channel_files("2.0.0"), vec!["latest.yml"]);
        assert_eq!(channel_files("2.0.0-beta.1"), vec!["latest.yml", "beta.yml"]);
        assert_eq!(channel_files("2.0.0-rc.2"), vec!["latest.yml", "rc.yml"]);
        assert_eq!(prerelease_channel("2.0.0-1"), None);
    }

    #[test]
    fn latest_yml_layout() {
        let yml = bridge_latest_yml("2.0.0", "AWS-Profile-Manager-Setup-2.0.0.exe", b"abc", "2026-11-15T10:00:00.000Z");
        let sha = sha512_base64(b"abc");
        assert_eq!(
            yml,
            format!(
                "version: 2.0.0\nfiles:\n  - url: AWS-Profile-Manager-Setup-2.0.0.exe\n    sha512: {sha}\n    size: 3\npath: AWS-Profile-Manager-Setup-2.0.0.exe\nsha512: {sha}\nreleaseDate: '2026-11-15T10:00:00.000Z'\n"
            )
        );
        // The well-known SHA-512 of "abc", base64.
        assert!(sha.starts_with("3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw=="));
    }

    #[test]
    fn committed_public_key_is_a_minisign_key() {
        let line = public_key_line(COMMITTED_PUBLIC_KEY).expect("key line");
        PublicKey::from_base64(line).expect("valid minisign public key");
    }
}
