//! Feeds made by apm-release are accepted by the app's own updater code: `parse_manifest` finds the
//! platform entry and `verify_installer` (sha256 + minisign-verify) accepts the installer, and
//! refuses a changed one. The real release key is checked against the public key compiled into
//! apm-core when it is present on this machine (it never is in CI unless the secret is set).

use apm_core::extras::updater::{parse_manifest, platform_key, verify_installer};
use apm_release::*;

fn throwaway_key() -> (minisign::SecretKey, String) {
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let pk = kp.pk.to_box().unwrap().into_string();
    (kp.sk, pk)
}

#[test]
fn manifest_verifies_with_the_core_updater() {
    let (sk, pk) = throwaway_key();
    let installer = b"MZ pretend installer bytes".repeat(1000);
    let file = installer_file_name("2.0.1");
    let url = github_download_url("2.0.1", &file);
    let entry = PlatformEntry::new("windows-x86_64", &url, &installer, &sk, &file, "2.0.1").unwrap();
    let manifest = update_manifest("2.0.1", &[entry], None).unwrap();

    // Round trip through text, as the app downloads it.
    let manifest: serde_json::Value = serde_json::from_str(&serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    let (version, asset) = parse_manifest(&manifest, "windows-x86_64").expect("platform entry");
    assert_eq!(version, "2.0.1");
    assert_eq!(asset.url, "https://github.com/MichaelGermino/AWSProfileManager/releases/download/v2.0.1/AWS-Profile-Manager-Setup-2.0.1.exe");
    verify_installer(&installer, &asset, &pk).expect("verifies with the full .pub file");
    verify_installer(&installer, &asset, public_key_line(&pk).unwrap()).expect("verifies with the bare key line");

    let mut tampered = installer.clone();
    tampered[0] ^= 1;
    assert!(verify_installer(&tampered, &asset, &pk).is_err(), "a changed installer is refused");
    let (_, other_pk) = throwaway_key();
    assert!(verify_installer(&installer, &asset, &other_pk).is_err(), "another key is refused");
}

#[test]
fn merge_keeps_other_platforms() {
    let (sk, _) = throwaway_key();
    let win = PlatformEntry::new("windows-x86_64", "https://x/w.exe", b"w", &sk, "w.exe", "2.0.1").unwrap();
    let first = update_manifest("2.0.1", &[win], None).unwrap();
    let mac = PlatformEntry::new("darwin-aarch64", "https://x/m.tar.gz", b"m", &sk, "m.tar.gz", "2.0.1").unwrap();
    let merged = update_manifest("2.0.1", &[mac], Some(&first)).unwrap();
    assert!(parse_manifest(&merged, "windows-x86_64").is_some());
    assert!(parse_manifest(&merged, "darwin-aarch64").is_some());
    assert!(update_manifest("2.0.2", &[], Some(&first)).is_err());
}

#[test]
fn committed_key_is_the_one_the_app_trusts() {
    let compiled = apm_core::core::CoreConfig::for_user("0.0.0").update_public_key.expect("release builds carry a key");
    assert_eq!(public_key_line(&compiled), public_key_line(COMMITTED_PUBLIC_KEY));
    assert!(platform_key().starts_with("windows") || platform_key().starts_with("darwin") || platform_key().starts_with("linux"));
}

/// With the real secret key on this machine (%USERPROFILE%\.apm-release), a signature made with it
/// verifies against the key compiled into the app. Skipped (passes) when the key is absent.
#[test]
fn real_release_key_matches_the_compiled_key() {
    let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()).join(".apm-release");
    let (Ok(text), Ok(password)) = (std::fs::read_to_string(dir.join("updater.key")), std::fs::read_to_string(dir.join("updater.key.password.txt"))) else {
        eprintln!("skipped: no release key in {}", dir.display());
        return;
    };
    let sk = load_secret_key(&text, password.trim()).expect("release key opens");
    let installer = b"release key check".to_vec();
    let entry = PlatformEntry::new(platform_key(), "https://x/i.exe", &installer, &sk, "i.exe", "9.9.9").unwrap();
    let manifest = update_manifest("9.9.9", &[entry], None).unwrap();
    let (_, asset) = parse_manifest(&manifest, platform_key()).unwrap();
    let compiled = apm_core::core::CoreConfig::for_user("0.0.0").update_public_key.unwrap();
    verify_installer(&installer, &asset, &compiled).expect("the release key signs what the app accepts");
}
